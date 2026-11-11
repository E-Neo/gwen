//! Interactive browser editor for a gwen project.
//!
//! TOML stays the single source of truth. The browser receives a rendered deck
//! plus shape metadata, sends semantic actions, and the server applies each
//! action to the TOML with `toml_edit`, validates the project and re-renders.

mod actions;
mod server;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use miette::{Result, miette};
use serde::Serialize;
use serde_json::{Value, json};

use crate::defaults::{self, Defaults};
use crate::fonts::Fonts;
use crate::model::{Project, Slide};
use crate::render;
use crate::scene;
use crate::svg;

pub use actions::{Action, Outcome};
pub use server::serve;

const HISTORY_LIMIT: usize = 100;

/// One editor session for a project directory.
pub struct Editor {
    dir: PathBuf,
    revision: u64,
    view: DeckView,
    history: Vec<Snapshot>,
    cursor: usize,
    known: BTreeMap<PathBuf, Vec<u8>>,
    fonts: Fonts,
    defaults: std::sync::Arc<defaults::Defaults>,
}

#[derive(Clone, Default)]
struct Snapshot {
    files: BTreeMap<String, String>,
}

#[derive(Clone, Serialize)]
struct DeckView {
    title: String,
    width: f64,
    height: f64,
    sections: Vec<SectionView>,
    slides: Vec<SlideView>,
}

#[derive(Clone, Serialize)]
struct SectionView {
    title: String,
    slides: Vec<usize>,
}

#[derive(Clone, Serialize)]
struct SlideView {
    index: usize,
    file: String,
    stem: String,
    section: usize,
    width: f64,
    height: f64,
    svg: String,
    background: Value,
    notes: Option<String>,
    shapes: Vec<ShapeView>,
}

#[derive(Clone, Serialize)]
struct ShapeView {
    index: usize,
    kind: String,
    #[serde(rename = "type")]
    ty: String,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    x_raw: Option<String>,
    y_raw: Option<String>,
    w_raw: Option<String>,
    h_raw: Option<String>,
    rotate: f64,
    flip_h: bool,
    flip_v: bool,
    placeholder: Option<String>,
    move_locked: bool,
    resize_locked: bool,
    text: Option<String>,
    paragraphs: Vec<ParagraphView>,
    effective: Value,
    image_src: Option<String>,
}

#[derive(Clone, Serialize)]
struct ParagraphView {
    text: String,
    options: Value,
}

impl Editor {
    /// Open a project, render it, and prepare the initial history snapshot.
    pub fn open(dir: &Path) -> Result<Self> {
        let dir = dir.to_path_buf();
        let project = Project::load(&dir)?;
        let fonts = Fonts::load(&dir);
        let defaults = defaults::get(
            &project.main.theme.major_font,
            &project.main.theme.minor_font,
        )?;
        let view = build_view(&dir, &project, &fonts, &defaults)?;
        let known = read_known_files(&dir)?;
        let mut editor = Editor {
            dir,
            revision: 1,
            view,
            history: Vec::new(),
            cursor: 0,
            known,
            fonts,
            defaults,
        };
        let snapshot = editor.snapshot()?;
        editor.history.push(snapshot);
        Ok(editor)
    }

    pub fn project_json(&self) -> Value {
        json!({
            "revision": self.revision,
            "title": self.view.title,
            "width": self.view.width,
            "height": self.view.height,
            "sections": self.view.sections,
            "slides": self.view.slides.iter().map(|s| json!({
                "index": s.index,
                "file": s.file,
                "stem": s.stem,
                "section": s.section,
            })).collect::<Vec<_>>(),
            "canUndo": self.cursor > 0,
            "canRedo": self.cursor + 1 < self.history.len(),
        })
    }

    pub fn slide_json(&self, index: usize) -> Result<Value> {
        let slide = self
            .view
            .slides
            .get(index)
            .ok_or_else(|| miette!("slide {index} does not exist"))?;
        Ok(json!({
            "revision": self.revision,
            "slide": slide,
        }))
    }

    pub fn svg(&self, index: usize) -> Result<&str> {
        self.view
            .slides
            .get(index)
            .map(|slide| slide.svg.as_str())
            .ok_or_else(|| miette!("slide {index} does not exist"))
    }

    pub fn present_json(&self) -> Value {
        json!({
            "title": self.view.title,
            "width": self.view.width,
            "height": self.view.height,
            "slides": self.view.slides.iter().map(|s| &s.svg).collect::<Vec<_>>(),
        })
    }

    /// Apply one semantic action, validate, re-render and record history.
    pub fn apply(&mut self, action: &Action) -> Result<Value> {
        self.check_disk()?;
        let before = self.snapshot()?;
        let outcome = match actions::apply(&self.dir, action) {
            Ok(outcome) => outcome,
            Err(err) => {
                self.restore(&before)?;
                self.reload()?;
                return Err(err);
            }
        };
        let single_slide = matches!(
            action,
            Action::Move { .. }
                | Action::Resize { .. }
                | Action::SetShape { .. }
                | Action::SetGeometry { .. }
                | Action::SetParagraph { .. }
                | Action::AddText { .. }
                | Action::AddShape { .. }
                | Action::AddImage { .. }
                | Action::ReplaceImage { .. }
                | Action::Align { .. }
                | Action::Distribute { .. }
                | Action::SetSlide { .. }
                | Action::DeleteShapes { .. }
                | Action::DuplicateShapes { .. }
                | Action::ZOrder { .. }
        );
        let refreshed = if single_slide {
            match outcome.slide {
                Some(index) => self.refresh_slide(index),
                None => self.reload().map(|_| ()),
            }
        } else {
            self.reload().map(|_| ())
        };
        if let Err(err) = refreshed {
            self.restore(&before)?;
            let _ = self.reload();
            return Err(err);
        }
        self.push_history()?;
        Ok(self.action_response(outcome))
    }

    pub fn undo(&mut self) -> Result<Value> {
        self.check_disk()?;
        if self.cursor == 0 {
            return Err(miette!("nothing to undo"));
        }
        self.cursor -= 1;
        let snapshot = self.history[self.cursor].clone();
        self.restore(&snapshot)?;
        self.reload()?;
        Ok(self.action_response(actions::Outcome::default()))
    }

    pub fn redo(&mut self) -> Result<Value> {
        self.check_disk()?;
        if self.cursor + 1 >= self.history.len() {
            return Err(miette!("nothing to redo"));
        }
        self.cursor += 1;
        let snapshot = self.history[self.cursor].clone();
        self.restore(&snapshot)?;
        self.reload()?;
        Ok(self.action_response(actions::Outcome::default()))
    }

    /// Read-only check used by the browser auto-reload poll. Does not rebuild
    /// the deck or touch history.
    pub fn disk_status(&self) -> Result<Value> {
        let now = read_known_files(&self.dir)?;
        Ok(json!({
            "revision": self.revision,
            "changed": now != self.known,
        }))
    }

    pub fn reload(&mut self) -> Result<Value> {
        let project = Project::load(&self.dir)?;
        self.view = build_view(&self.dir, &project, &self.fonts, &self.defaults)?;
        self.known = read_known_files(&self.dir)?;
        self.revision += 1;
        Ok(self.project_json())
    }

    /// Discard history and reload from disk after an external edit.
    pub fn reload_external(&mut self) -> Result<Value> {
        let project = Project::load(&self.dir)?;
        self.fonts = Fonts::load(&self.dir);
        self.defaults = defaults::get(
            &project.main.theme.major_font,
            &project.main.theme.minor_font,
        )?;
        self.reload()?;
        let snapshot = self.snapshot()?;
        self.history.clear();
        self.history.push(snapshot);
        self.cursor = 0;
        Ok(self.project_json())
    }

    /// Rebuild only one slide after a shape-level action. Section actions and
    /// slide structure changes still go through `reload`.
    fn refresh_slide(&mut self, index: usize) -> Result<()> {
        let project = Project::load(&self.dir)?;
        let spec_text = render::spec_json(&project)?;
        let spec: Value = serde_json::from_str(&spec_text)
            .map_err(|e| miette!("the built spec is invalid JSON: {e}"))?;
        let slide_value = spec
            .get("sections")
            .and_then(Value::as_array)
            .and_then(|sections| {
                sections
                    .iter()
                    .flat_map(|section| {
                        section
                            .get("slides")
                            .and_then(Value::as_array)
                            .map(|slides| slides.iter())
                            .into_iter()
                            .flatten()
                    })
                    .nth(index)
            })
            .cloned()
            .ok_or_else(|| miette!("slide {index} was not rendered"))?;
        let file = slide_value
            .get("file")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let source = load_slide(&project, &file)?;
        let width = spec.get("width").and_then(Value::as_f64).unwrap_or(13.333);
        let height = spec.get("height").and_then(Value::as_f64).unwrap_or(7.5);
        let mut mini = spec.clone();
        mini["sections"] = json!([{ "title": "", "slides": [slide_value.clone()] }]);
        let scenes = scene::build(&mini, &self.defaults, &self.dir, &self.fonts)
            .map_err(|e| miette!("{e}"))?;
        let scene = scenes
            .first()
            .ok_or_else(|| miette!("slide {index} was not rendered"))?;
        let svg = svg::render(scene, &self.fonts);
        let shapes = shape_views(&slide_value, &source, scene);
        let stem = Path::new(&file)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("slide-{}", index + 1));
        let section = self
            .view
            .slides
            .get(index)
            .map(|slide| slide.section)
            .unwrap_or(0);
        let Some(slot) = self.view.slides.get_mut(index) else {
            return Err(miette!("slide {index} does not exist"));
        };
        *slot = SlideView {
            index,
            file,
            stem,
            section,
            width,
            height,
            svg,
            background: slide_value
                .get("background")
                .cloned()
                .unwrap_or(Value::Null),
            notes: slide_value
                .get("notes")
                .and_then(Value::as_str)
                .map(String::from),
            shapes,
        };
        self.known = read_known_files(&self.dir)?;
        self.revision += 1;
        Ok(())
    }

    fn action_response(&self, outcome: actions::Outcome) -> Value {
        let slide = outcome
            .slide
            .map(|slide| slide.min(self.view.slides.len().saturating_sub(1)));
        json!({
            "revision": self.revision,
            "slide": slide,
            "section": outcome.section,
            "selection": outcome.select,
            "project": self.project_json(),
        })
    }

    fn check_disk(&self) -> Result<()> {
        let now = read_known_files(&self.dir)?;
        if now != self.known {
            return Err(miette!(
                "the project changed on disk; reload before editing"
            ));
        }
        Ok(())
    }

    fn push_history(&mut self) -> Result<()> {
        let snapshot = self.snapshot()?;
        if self.cursor + 1 < self.history.len() {
            self.history.truncate(self.cursor + 1);
        }
        self.history.push(snapshot);
        if self.history.len() > HISTORY_LIMIT {
            self.history.remove(0);
        }
        self.cursor = self.history.len() - 1;
        Ok(())
    }

    fn snapshot(&self) -> Result<Snapshot> {
        let mut files = BTreeMap::new();
        let main = self.dir.join("main.toml");
        files.insert(
            "main.toml".to_string(),
            std::fs::read_to_string(&main)
                .map_err(|e| miette!("cannot read `{}`: {e}", main.display()))?,
        );
        let slides = self.dir.join("slides");
        if slides.is_dir() {
            for entry in std::fs::read_dir(&slides)
                .map_err(|e| miette!("cannot list `{}`: {e}", slides.display()))?
            {
                let entry =
                    entry.map_err(|e| miette!("cannot read `{}`: {e}", slides.display()))?;
                if !entry.file_type().is_ok_and(|t| t.is_file())
                    || !entry.file_name().to_string_lossy().ends_with(".toml")
                {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                let path = entry.path();
                let contents = std::fs::read_to_string(&path)
                    .map_err(|e| miette!("cannot read `{}`: {e}", path.display()))?;
                files.insert(format!("slides/{name}"), contents);
            }
        }
        Ok(Snapshot { files })
    }

    fn restore(&self, snapshot: &Snapshot) -> Result<()> {
        let slides = self.dir.join("slides");
        if slides.is_dir() {
            for entry in std::fs::read_dir(&slides)
                .map_err(|e| miette!("cannot list `{}`: {e}", slides.display()))?
            {
                let entry =
                    entry.map_err(|e| miette!("cannot read `{}`: {e}", slides.display()))?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if !name.ends_with(".toml") {
                    continue;
                }
                let rel = format!("slides/{name}");
                if !snapshot.files.contains_key(&rel) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        for (rel, contents) in &snapshot.files {
            let path = self.dir.join(rel);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| miette!("cannot create `{}`: {e}", parent.display()))?;
            }
            std::fs::write(&path, contents)
                .map_err(|e| miette!("cannot write `{}`: {e}", path.display()))?;
        }
        Ok(())
    }
}

fn read_known_files(dir: &Path) -> Result<BTreeMap<PathBuf, Vec<u8>>> {
    let mut out = BTreeMap::new();
    for sub in ["main.toml", "masters", "slides"] {
        let path = dir.join(sub);
        if path.is_file() {
            let bytes = std::fs::read(&path)
                .map_err(|e| miette!("cannot read `{}`: {e}", path.display()))?;
            out.insert(path, bytes);
        } else if path.is_dir() {
            for entry in std::fs::read_dir(&path)
                .map_err(|e| miette!("cannot list `{}`: {e}", path.display()))?
            {
                let entry = entry.map_err(|e| miette!("cannot read `{}`: {e}", path.display()))?;
                if !entry.file_type().is_ok_and(|t| t.is_file())
                    || !entry.file_name().to_string_lossy().ends_with(".toml")
                {
                    continue;
                }
                let p = entry.path();
                let bytes =
                    std::fs::read(&p).map_err(|e| miette!("cannot read `{}`: {e}", p.display()))?;
                out.insert(p, bytes);
            }
        }
    }
    Ok(out)
}

fn build_view(
    dir: &Path,
    project: &Project,
    fonts: &Fonts,
    defaults: &Defaults,
) -> Result<DeckView> {
    let spec_text = render::spec_json(project)?;
    let spec: Value = serde_json::from_str(&spec_text)
        .map_err(|e| miette!("the built spec is invalid JSON: {e}"))?;
    let scenes = scene::build(&spec, defaults, dir, fonts).map_err(|e| miette!("{e}"))?;

    let width = spec.get("width").and_then(Value::as_f64).unwrap_or(13.333);
    let height = spec.get("height").and_then(Value::as_f64).unwrap_or(7.5);
    let title = project.main.presentation.title.clone();
    let mut slides = Vec::new();
    let mut sections = Vec::new();
    for (section_index, section) in spec
        .get("sections")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .enumerate()
    {
        let section_title = section
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let mut section_slides = Vec::new();
        for slide in section
            .get("slides")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
        {
            let index = slides.len();
            let file = slide
                .get("file")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let scene = scenes
                .get(index)
                .ok_or_else(|| miette!("slide {index} was not rendered"))?;
            let svg = svg::render(scene, fonts);
            let source = load_slide(project, &file)?;
            let shapes = shape_views(slide, &source, scene);
            let stem = Path::new(&file)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| format!("slide-{}", index + 1));
            slides.push(SlideView {
                index,
                file,
                stem,
                section: section_index,
                width,
                height,
                svg,
                background: slide.get("background").cloned().unwrap_or(Value::Null),
                notes: slide.get("notes").and_then(Value::as_str).map(String::from),
                shapes,
            });
            section_slides.push(index);
        }
        sections.push(SectionView {
            title: section_title,
            slides: section_slides,
        });
    }
    Ok(DeckView {
        title,
        width,
        height,
        sections,
        slides,
    })
}

fn load_slide(project: &Project, file: &str) -> Result<Slide> {
    let path = project.slide_path(file);
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| miette!("cannot read `{}`: {e}", path.display()))?;
    toml::from_str(&raw).map_err(|e| miette!("cannot parse `{}`: {e}", path.display()))
}

fn shape_views(slide: &Value, source: &Slide, scene: &scene::Scene) -> Vec<ShapeView> {
    let mut out = Vec::new();
    let Some(shapes) = slide.get("shapes").and_then(Value::as_array) else {
        return out;
    };
    for (index, shape) in shapes.iter().enumerate() {
        let source_shape = source.shapes.get(index);
        let placeholder = source_shape
            .and_then(|s| s.opts.get("placeholder"))
            .and_then(|v| v.as_str())
            .map(String::from);
        let kind = shape
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("text")
            .to_string();
        let ty = source_shape
            .map(|s| s.ty.clone())
            .or_else(|| shape.get("type").and_then(Value::as_str).map(String::from))
            .unwrap_or_else(|| kind.clone());
        let mut rect: Option<scene::Rect> = None;
        let mut rotate = 0.0;
        let mut flip_h = false;
        let mut flip_v = false;
        for item in scene.items.iter().filter(|item| item.source == Some(index)) {
            let r = item.rect;
            rect = Some(match rect {
                Some(acc) => {
                    let x = acc.x.min(r.x);
                    let y = acc.y.min(r.y);
                    let right = (acc.x + acc.w).max(r.x + r.w);
                    let bottom = (acc.y + acc.h).max(r.y + r.h);
                    scene::Rect {
                        x,
                        y,
                        w: right - x,
                        h: bottom - y,
                    }
                }
                None => r,
            });
            rotate = item.rotate;
            flip_h = item.flip_h;
            flip_v = item.flip_v;
        }
        let rect = rect.unwrap_or(scene::Rect {
            x: shape.get("x").and_then(Value::as_f64).unwrap_or(0.0),
            y: shape.get("y").and_then(Value::as_f64).unwrap_or(0.0),
            w: shape.get("w").and_then(Value::as_f64).unwrap_or(0.0),
            h: shape.get("h").and_then(Value::as_f64).unwrap_or(0.0),
        });
        let mut effective = shape.clone();
        if let Some(obj) = effective.as_object_mut() {
            for key in [
                "index", "kind", "type", "x", "y", "w", "h", "runs", "rows", "preview", "data",
                "mime",
            ] {
                obj.remove(key);
            }
        }
        let paragraphs = source_shape
            .map(|s| {
                s.paragraphs
                    .iter()
                    .map(|para| ParagraphView {
                        text: para.text.clone(),
                        options: serde_json::to_value(&para.opts).unwrap_or(Value::Null),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let text = source_shape.and_then(|s| s.text.clone());
        let image_src = source_shape.and_then(|s| s.src.clone());
        let raw = |coord: Option<&crate::units::Coord>| {
            coord.map(|coord| match coord {
                crate::units::Coord::Emu(emu) => emu.to_string(),
                crate::units::Coord::Text(text) => text.clone(),
            })
        };
        let x_raw = raw(source_shape.and_then(|s| s.x.as_ref()));
        let y_raw = raw(source_shape.and_then(|s| s.y.as_ref()));
        let w_raw = raw(source_shape.and_then(|s| s.w.as_ref()));
        let h_raw = raw(source_shape.and_then(|s| s.h.as_ref()));
        let resize_locked = placeholder.is_some() || kind == "table";
        out.push(ShapeView {
            index,
            kind,
            ty,
            x: rect.x,
            y: rect.y,
            w: rect.w,
            h: rect.h,
            x_raw,
            y_raw,
            w_raw,
            h_raw,
            rotate,
            flip_h,
            flip_v,
            placeholder: placeholder.clone(),
            move_locked: placeholder.is_some(),
            resize_locked,
            text,
            paragraphs,
            effective,
            image_src,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn project(name: &str) -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "gwen-edit-{}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst),
            name
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("slides")).unwrap();
        std::fs::create_dir_all(dir.join("masters")).unwrap();
        std::fs::write(
            dir.join("main.toml"),
            "[presentation]\ntitle = \"deck\"\nwidth = \"10in\"\nheight = \"7.5in\"\n\n[theme]\nmajor_font = \"Arial\"\nminor_font = \"Arial\"\n\n[[sections]]\ntitle = \"S\"\nslides = [\"a.toml\", \"b.toml\"]\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("masters").join("base.toml"),
            "background = { color = \"FFFFFF\" }\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("slides").join("a.toml"),
            "# keep this comment\nmaster = \"base\"\n\n[[shapes]]\ntype = \"text\"\nx = 914400\ny = \"1.25in\"\nw = \"3in\"\nh = \"1in\"\ntext = \"hello\"\nfont_size = 20\n\n[[shapes]]\ntype = \"rect\"\nx = \"1in\"\ny = \"3in\"\nw = \"2in\"\nh = \"1in\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("slides").join("b.toml"),
            "master = \"base\"\n\n[[shapes]]\ntype = \"text\"\nx = \"1in\"\ny = \"1in\"\nw = \"2in\"\nh = \"1in\"\ntext = \"second\"\n",
        )
        .unwrap();
        dir
    }

    fn slide_text(dir: &Path, file: &str) -> String {
        std::fs::read_to_string(dir.join("slides").join(file)).unwrap()
    }

    #[test]
    fn move_preserves_units_comments_and_undoes_exactly() {
        let dir = project("move");
        let before = slide_text(&dir, "a.toml");
        let mut editor = Editor::open(&dir).unwrap();
        editor
            .apply(&Action::Move {
                slide: 0,
                indices: vec![0],
                dx: 0.5,
                dy: 0.25,
            })
            .unwrap();
        let after = slide_text(&dir, "a.toml");
        assert!(after.contains("x = 1371600"), "{after}");
        assert!(after.contains("y = \"1.5in\""), "{after}");
        assert!(after.contains("# keep this comment"), "{after}");
        assert_ne!(before, after);
        editor.undo().unwrap();
        assert_eq!(before, slide_text(&dir, "a.toml"));
        editor.redo().unwrap();
        assert_eq!(after, slide_text(&dir, "a.toml"));
    }

    #[test]
    fn set_geometry_accepts_units() {
        let dir = project("geometry-units");
        let mut editor = Editor::open(&dir).unwrap();

        let slide = editor.slide_json(0).unwrap();
        assert_eq!(
            slide["slide"]["shapes"][0]["x_raw"].as_str(),
            Some("914400")
        );
        assert_eq!(
            slide["slide"]["shapes"][0]["y_raw"].as_str(),
            Some("1.25in")
        );

        for (key, value) in [("x", "2cm"), ("w", "72pt"), ("y", "914400"), ("h", "50%")] {
            editor
                .apply(&Action::SetGeometry {
                    slide: 0,
                    index: 0,
                    key: key.into(),
                    value: value.into(),
                })
                .unwrap();
        }

        let doc: toml::Value = toml::from_str(&slide_text(&dir, "a.toml")).unwrap();
        let shape = &doc["shapes"][0];
        assert_eq!(shape["x"].as_str(), Some("2cm"));
        assert_eq!(shape["w"].as_str(), Some("72pt"));
        assert_eq!(shape["y"].as_integer(), Some(914_400));
        assert_eq!(shape["h"].as_str(), Some("50%"));

        let slide = editor.slide_json(0).unwrap();
        let source = &slide["slide"]["shapes"][0];
        assert_eq!(source["x_raw"].as_str(), Some("2cm"));
        assert!((source["x"].as_f64().unwrap() - 2.0 / 2.54).abs() < 1e-6);

        let before = slide_text(&dir, "a.toml");
        assert!(
            editor
                .apply(&Action::SetGeometry {
                    slide: 0,
                    index: 0,
                    key: "x".into(),
                    value: "bogus".into(),
                })
                .is_err()
        );
        assert!(
            editor
                .apply(&Action::SetGeometry {
                    slide: 0,
                    index: 0,
                    key: "x".into(),
                    value: "-1cm".into(),
                })
                .is_err()
        );
        assert_eq!(before, slide_text(&dir, "a.toml"));
    }

    #[test]
    fn invalid_resize_rolls_back() {
        let dir = project("resize");
        let before = slide_text(&dir, "a.toml");
        let mut editor = Editor::open(&dir).unwrap();
        assert!(
            editor
                .apply(&Action::Resize {
                    slide: 0,
                    index: 1,
                    x: 1.0,
                    y: 3.0,
                    w: -1.0,
                    h: 1.0,
                })
                .is_err()
        );
        assert_eq!(before, slide_text(&dir, "a.toml"));
    }

    #[test]
    fn set_shape_edits_text_and_styles() {
        let dir = project("set");
        let mut editor = Editor::open(&dir).unwrap();
        let mut keys = BTreeMap::new();
        keys.insert("text".to_string(), json!("**hi**"));
        keys.insert("bold".to_string(), json!(true));
        keys.insert("font_size".to_string(), json!(28));
        editor
            .apply(&Action::SetShape {
                slide: 0,
                index: 0,
                keys,
            })
            .unwrap();
        let xml = slide_text(&dir, "a.toml");
        assert!(xml.contains("text = \"**hi**\""), "{xml}");
        assert!(xml.contains("bold = true"), "{xml}");
        assert!(xml.contains("font_size = 28"), "{xml}");
    }

    #[test]
    fn shape_structure_actions_work() {
        let dir = project("shapes");
        let mut editor = Editor::open(&dir).unwrap();
        editor
            .apply(&Action::DuplicateShapes {
                slide: 0,
                indices: vec![1],
            })
            .unwrap();
        assert_eq!(slide_text(&dir, "a.toml").matches("[[shapes]]").count(), 3);
        editor
            .apply(&Action::ZOrder {
                slide: 0,
                indices: vec![2],
                op: "back".into(),
            })
            .unwrap();
        let doc: toml::Value = toml::from_str(&slide_text(&dir, "a.toml")).unwrap();
        let shapes = doc["shapes"].as_array().unwrap();
        assert_eq!(shapes[0]["type"].as_str(), Some("rect"), "{shapes:?}");
        editor
            .apply(&Action::DeleteShapes {
                slide: 0,
                indices: vec![0],
            })
            .unwrap();
        assert_eq!(slide_text(&dir, "a.toml").matches("[[shapes]]").count(), 2);
    }

    #[test]
    fn slide_structure_actions_work() {
        let dir = project("slides");
        let mut editor = Editor::open(&dir).unwrap();
        editor
            .apply(&Action::AddSlide {
                after: Some(0),
                section: None,
            })
            .unwrap();
        assert_eq!(editor.project_json()["slides"].as_array().unwrap().len(), 3);
        let added = editor.project_json()["slides"][1]["file"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(dir.join("slides").join(&added).is_file());
        editor.apply(&Action::DuplicateSlide { slide: 0 }).unwrap();
        assert_eq!(editor.project_json()["slides"].as_array().unwrap().len(), 4);
        editor
            .apply(&Action::MoveSlide {
                from: 0,
                to: 2,
                section: None,
            })
            .unwrap();
        editor.apply(&Action::DeleteSlide { slide: 1 }).unwrap();
        assert_eq!(editor.project_json()["slides"].as_array().unwrap().len(), 3);
        editor.undo().unwrap();
        assert_eq!(editor.project_json()["slides"].as_array().unwrap().len(), 4);
    }

    #[test]
    fn disk_status_detects_external_changes() {
        let dir = project("disk-status");
        let mut editor = Editor::open(&dir).unwrap();
        assert_eq!(
            editor.disk_status().unwrap()["changed"].as_bool(),
            Some(false)
        );
        std::fs::write(
            dir.join("slides").join("a.toml"),
            "master = \"base\"\n\n[[shapes]]\ntype = \"text\"\ntext = \"changed\"\n",
        )
        .unwrap();
        assert_eq!(
            editor.disk_status().unwrap()["changed"].as_bool(),
            Some(true)
        );
        editor.reload_external().unwrap();
        assert_eq!(
            editor.disk_status().unwrap()["changed"].as_bool(),
            Some(false)
        );
    }

    #[test]
    fn external_changes_are_rejected() {
        let dir = project("external");
        let mut editor = Editor::open(&dir).unwrap();
        std::fs::write(
            dir.join("main.toml"),
            "[presentation]\ntitle = \"manual\"\n",
        )
        .unwrap();
        let err = editor
            .apply(&Action::Move {
                slide: 0,
                indices: vec![0],
                dx: 1.0,
                dy: 0.0,
            })
            .unwrap_err();
        assert!(format!("{err:?}").contains("changed on disk"), "{err:?}");
    }

    #[test]
    fn add_text_and_external_reload() {
        let dir = project("add-text");
        let mut editor = Editor::open(&dir).unwrap();
        editor
            .apply(&Action::AddText {
                slide: 0,
                x: 1.0,
                y: 5.0,
                w: 4.0,
                h: 1.0,
                text: Some("new text".into()),
            })
            .unwrap();
        let xml = slide_text(&dir, "a.toml");
        assert!(xml.contains("text = \"new text\""), "{xml}");
        assert_eq!(
            editor.slide_json(0).unwrap()["slide"]["shapes"]
                .as_array()
                .unwrap()
                .len(),
            3
        );

        // An external edit is picked up by reload_external, which discards
        // history so undo cannot resurrect the old disk state.
        std::fs::write(
            dir.join("slides").join("a.toml"),
            "master = \"base\"\n\n[[shapes]]\ntype = \"text\"\nx = \"1in\"\ny = \"1in\"\nw = \"2in\"\nh = \"1in\"\ntext = \"external\"\n",
        )
        .unwrap();
        let project = editor.reload_external().unwrap();
        assert_eq!(project["canUndo"].as_bool(), Some(false));
        assert_eq!(
            editor.slide_json(0).unwrap()["slide"]["shapes"][0]["text"].as_str(),
            Some("external")
        );
    }

    fn inches(value: &toml::Value) -> f64 {
        match value {
            toml::Value::Integer(value) => *value as f64 / 914_400.0,
            toml::Value::Float(value) => *value,
            toml::Value::String(value) => value
                .trim()
                .trim_end_matches("in")
                .parse::<f64>()
                .unwrap_or_else(|_| panic!("unexpected coordinate `{value}`")),
            other => panic!("unexpected coordinate {other:?}"),
        }
    }

    #[test]
    fn add_shape_align_and_distribute() {
        let dir = project("align");
        let mut editor = Editor::open(&dir).unwrap();
        for (shape_type, x, y) in [
            ("rect", 1.0, 1.0),
            ("ellipse", 2.0, 2.0),
            ("triangle", 8.0, 3.0),
        ] {
            editor
                .apply(&Action::AddShape {
                    slide: 0,
                    shape_type: shape_type.into(),
                    x,
                    y,
                    w: 2.0,
                    h: 1.0,
                    text: None,
                })
                .unwrap();
        }
        let doc: toml::Value = toml::from_str(&slide_text(&dir, "a.toml")).unwrap();
        let shapes = doc["shapes"].as_array().unwrap();
        assert_eq!(shapes.len(), 5);
        assert_eq!(shapes[4]["type"].as_str(), Some("triangle"));

        editor
            .apply(&Action::Align {
                slide: 0,
                indices: vec![2, 3, 4],
                edge: "left".into(),
            })
            .unwrap();
        let doc: toml::Value = toml::from_str(&slide_text(&dir, "a.toml")).unwrap();
        let shapes = doc["shapes"].as_array().unwrap();
        let xs: Vec<f64> = shapes[2..]
            .iter()
            .map(|shape| inches(&shape["x"]))
            .collect();
        assert!(xs.iter().all(|x| (x - 1.0).abs() < 1e-6), "{xs:?}");

        editor.undo().unwrap();
        editor
            .apply(&Action::Distribute {
                slide: 0,
                indices: vec![2, 3, 4],
                axis: "horizontal".into(),
            })
            .unwrap();
        let doc: toml::Value = toml::from_str(&slide_text(&dir, "a.toml")).unwrap();
        let shapes = doc["shapes"].as_array().unwrap();
        let xs: Vec<f64> = shapes[2..]
            .iter()
            .map(|shape| inches(&shape["x"]))
            .collect();
        // Shapes are 2in wide, so equal spacing leaves a 1.5in gap: 1, 4.5, 8.
        assert!((xs[0] - 1.0).abs() < 1e-6, "{xs:?}");
        assert!((xs[1] - 4.5).abs() < 1e-6, "{xs:?}");
        assert!((xs[2] - 8.0).abs() < 1e-6, "{xs:?}");

        editor.undo().unwrap();
        let doc: toml::Value = toml::from_str(&slide_text(&dir, "a.toml")).unwrap();
        let shapes = doc["shapes"].as_array().unwrap();
        assert!((inches(&shapes[3]["x"]) - 2.0).abs() < 1e-6);
    }

    #[test]
    fn add_and_replace_image() {
        let dir = project("image");
        let mut editor = Editor::open(&dir).unwrap();
        let pixel = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
        editor
            .apply(&Action::AddImage {
                slide: 0,
                name: "pixel.png".into(),
                data: pixel.into(),
                x: None,
                y: None,
                w: None,
                h: None,
            })
            .unwrap();
        let doc: toml::Value = toml::from_str(&slide_text(&dir, "a.toml")).unwrap();
        let shapes = doc["shapes"].as_array().unwrap();
        let added = shapes.last().unwrap();
        assert_eq!(added["type"].as_str(), Some("image"));
        assert_eq!(added["src"].as_str(), Some("media/pixel.png"));
        assert!((inches(&added["w"]) - 4.5).abs() < 1e-6);
        assert!((inches(&added["h"]) - 4.5).abs() < 1e-6);
        assert!(dir.join("media").join("pixel.png").is_file());

        editor
            .apply(&Action::ReplaceImage {
                slide: 0,
                index: shapes.len() - 1,
                name: "replacement.png".into(),
                data: pixel.into(),
            })
            .unwrap();
        let doc: toml::Value = toml::from_str(&slide_text(&dir, "a.toml")).unwrap();
        let shapes = doc["shapes"].as_array().unwrap();
        assert_eq!(
            shapes.last().unwrap()["src"].as_str(),
            Some("media/replacement.png")
        );
        assert!(dir.join("media").join("pixel.png").is_file());
        assert!(dir.join("media").join("replacement.png").is_file());
    }

    #[test]
    fn set_slide_background_and_notes() {
        let dir = project("slide-settings");
        let mut editor = Editor::open(&dir).unwrap();
        editor
            .apply(&Action::SetSlide {
                slide: 0,
                background: Some(Some(json!("#ff0000"))),
                notes: Some("hello\nworld".into()),
            })
            .unwrap();
        let doc: toml::Value = toml::from_str(&slide_text(&dir, "a.toml")).unwrap();
        assert_eq!(doc["background"].as_str(), Some("#ff0000"));
        assert_eq!(doc["notes"].as_str(), Some("hello\nworld"));
        let slide = editor.slide_json(0).unwrap();
        assert_eq!(
            slide["slide"]["background"]["color"].as_str(),
            Some("#ff0000")
        );
        assert_eq!(slide["slide"]["notes"].as_str(), Some("hello\nworld"));

        editor
            .apply(&Action::SetSlide {
                slide: 0,
                background: Some(None),
                notes: Some(String::new()),
            })
            .unwrap();
        let doc: toml::Value = toml::from_str(&slide_text(&dir, "a.toml")).unwrap();
        assert!(doc.get("background").is_none());
        assert!(doc.get("notes").is_none());
        let slide = editor.slide_json(0).unwrap();
        assert!(slide["slide"]["background"].is_null());
        assert!(slide["slide"]["notes"].is_null());
    }

    #[test]
    fn section_actions_add_rename_move_and_delete() {
        let dir = project("sections");
        let mut editor = Editor::open(&dir).unwrap();

        editor
            .apply(&Action::AddSection {
                after: Some(0),
                title: Some("Second".into()),
            })
            .unwrap();
        let project = editor.project_json();
        let sections = project["sections"].as_array().unwrap();
        assert_eq!(sections.len(), 2, "{sections:?}");
        assert_eq!(sections[1]["title"].as_str(), Some("Second"));
        assert!(sections[1]["slides"].as_array().unwrap().is_empty());

        editor
            .apply(&Action::RenameSection {
                section: 1,
                title: "Renamed".into(),
            })
            .unwrap();
        assert_eq!(
            editor.project_json()["sections"][1]["title"].as_str(),
            Some("Renamed")
        );

        // Add a slide into the empty section.
        editor
            .apply(&Action::AddSlide {
                after: None,
                section: Some(1),
            })
            .unwrap();
        assert_eq!(
            editor.project_json()["sections"][1]["slides"]
                .as_array()
                .unwrap()
                .len(),
            1
        );

        // Move a slide from section 0 into section 1.
        editor
            .apply(&Action::MoveSlide {
                from: 0,
                to: 0,
                section: Some(1),
            })
            .unwrap();
        let project = editor.project_json();
        assert_eq!(
            project["sections"][0]["slides"].as_array().unwrap().len(),
            1
        );
        assert_eq!(
            project["sections"][1]["slides"].as_array().unwrap().len(),
            2
        );

        // Deleting a section keeps its remaining slides by moving them into
        // the surviving section.
        editor.apply(&Action::DeleteSection { section: 0 }).unwrap();
        let project = editor.project_json();
        assert_eq!(project["sections"].as_array().unwrap().len(), 1);
        assert_eq!(project["slides"].as_array().unwrap().len(), 3);
        assert_eq!(
            project["sections"][0]["slides"].as_array().unwrap().len(),
            3
        );

        editor.undo().unwrap();
        assert_eq!(
            editor.project_json()["sections"].as_array().unwrap().len(),
            2
        );
    }

    #[test]
    fn section_drag_targets_are_addressable_after_reorder() {
        let dir = project("sections-drag");
        let mut editor = Editor::open(&dir).unwrap();
        editor
            .apply(&Action::AddSection {
                after: Some(0),
                title: Some("Empty".into()),
            })
            .unwrap();
        // Moving the first slide of section 0 to the start of the empty
        // section must work even though that section has no slide index.
        editor
            .apply(&Action::MoveSlide {
                from: 0,
                to: 0,
                section: Some(1),
            })
            .unwrap();
        let project = editor.project_json();
        assert_eq!(
            project["sections"][0]["slides"].as_array().unwrap().len(),
            1
        );
        assert_eq!(
            project["sections"][1]["slides"].as_array().unwrap().len(),
            1
        );
    }

    #[test]
    fn slide_metadata_has_source_bounds() {
        let dir = project("metadata");
        let editor = Editor::open(&dir).unwrap();
        let value = editor.slide_json(0).unwrap();
        let shapes = value["slide"]["shapes"].as_array().unwrap();
        assert_eq!(shapes.len(), 2);
        assert_eq!(shapes[0]["index"].as_u64(), Some(0));
        assert!((value["slide"]["width"].as_f64().unwrap() - 10.0).abs() < 1e-6);
        assert!((value["slide"]["height"].as_f64().unwrap() - 7.5).abs() < 1e-6);
        assert_eq!(shapes[0]["text"].as_str(), Some("hello"));
        assert!((shapes[0]["x"].as_f64().unwrap() - 1.0).abs() < 1e-6);
        assert!((shapes[0]["y"].as_f64().unwrap() - 1.25).abs() < 1e-6);
    }
}
