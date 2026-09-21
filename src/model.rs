//! TOML model for a gwen deck, mirroring the pptxgenjs API surface.
//!
//! `main.toml` (`[presentation]`, `[layout]`, `[theme]`, `[[sections]]`,
//! `[defaults.<type>]`, `[styles.<name>]`), `masters/<name>.toml` and
//! `slides/<name>.toml`. Option keys are snake_case and become the corresponding
//! pptxgenjs camelCase properties; arbitrary extra keys pass straight through.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use miette::{Result, miette};

use crate::units::Coord;

/// A free-form option map: everything pptxgenjs accepts for a call, in TOML.
pub type Opts = toml::Table;

/// The deck root, holding parsed `main.toml` plus the file layout.
pub struct Project {
    pub dir: PathBuf,
    pub main: Main,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Main {
    pub presentation: Presentation,
    #[serde(default)]
    pub layout: Layout,
    #[serde(default)]
    pub theme: Theme,
    #[serde(default)]
    pub sections: Vec<Section>,
    #[serde(default)]
    #[serde(rename = "defaults")]
    pub defaults: Defaults,
    #[serde(default)]
    pub styles: BTreeMap<String, Opts>,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Presentation {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub company: String,
    #[serde(default)]
    pub revision: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub rtl_mode: bool,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Layout {
    #[serde(default = "default_layout_name")]
    pub name: String,
    /// pptxgenjs `presLayout` width/height: EMU or a unit string/percentage.
    #[serde(default = "default_width", rename = "width")]
    pub width: Coord,
    #[serde(default = "default_height", rename = "height")]
    pub height: Coord,
}

fn default_layout_name() -> String {
    "GWEN".into()
}
fn default_width() -> Coord {
    Coord::Text("10in".into())
}
fn default_height() -> Coord {
    Coord::Text("7.5in".into())
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Theme {
    #[serde(default = "default_font")]
    pub major_font: String,
    #[serde(default = "default_font")]
    pub minor_font: String,
}

fn default_font() -> String {
    "Calibri".into()
}

fn default_theme() -> Theme {
    Theme {
        major_font: default_font(),
        minor_font: default_font(),
    }
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Defaults {
    #[serde(default)]
    pub text: Opts,
    #[serde(default)]
    pub shape: Opts,
    #[serde(default)]
    pub image: Opts,
}

/// The slide ordering index: each section pins `title` and the slide files.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Section {
    pub title: String,
    #[serde(default)]
    pub slides: Vec<String>,
}

/// `masters/<name>.toml`. The master name is the file stem; there is no
/// `title` field (the file name is the master name).
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Master {
    #[serde(default)]
    pub background: Option<toml::Value>,
    /// Margin in points/inches/EMU as a single number or `[t, r, b, l]`.
    #[serde(default)]
    pub margin: Option<toml::Value>,
    #[serde(default)]
    pub objects: Vec<Object>,
}

/// A master object (`rect`, `line`, `text`, `image`, `chart`, `placeholder`).
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Object {
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default)]
    pub style: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub src: Option<String>,
    #[serde(default)]
    pub x: Option<Coord>,
    #[serde(default)]
    pub y: Option<Coord>,
    #[serde(default)]
    pub w: Option<Coord>,
    #[serde(default)]
    pub h: Option<Coord>,
    #[serde(flatten)]
    pub opts: Opts,
}

/// `slides/<name>.toml`.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Slide {
    #[serde(default)]
    pub master: Option<String>,
    #[serde(default)]
    pub background: Option<toml::Value>,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub shapes: Vec<Shape>,
    #[serde(default)]
    pub notes: Option<String>,
}

/// A slide shape. `type` is `text`, `image` or a pptxgenjs `ShapeType` preset
/// (`rect`, `roundRect`, `line`, ...). Reserved `chart`, `table` and `media`
/// are recognised but not yet implemented.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Shape {
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default)]
    pub style: Option<String>,
    /// Shorthand for a single paragraph of markdown text.
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub paragraphs: Vec<Para>,
    #[serde(default)]
    pub src: Option<String>,
    #[serde(default)]
    pub x: Option<Coord>,
    #[serde(default)]
    pub y: Option<Coord>,
    #[serde(default)]
    pub w: Option<Coord>,
    #[serde(default)]
    pub h: Option<Coord>,
    #[serde(flatten)]
    pub opts: Opts,
}

/// An explicit paragraph: a markdown string plus paragraph-level pptxgenjs
/// options (`bullet`, `align`, `level`, ...).
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Para {
    pub text: String,
    #[serde(flatten)]
    pub opts: Opts,
}

/// Which pptxgenjs call a shape maps to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    Shape,
    Image,
}

impl Shape {
    pub fn kind(&self) -> Result<Kind, String> {
        match self.ty.as_str() {
            "text" => Ok(Kind::Text),
            "image" => Ok(Kind::Image),
            "chart" | "table" | "media" => {
                Err(format!("shape type `{}` is not supported yet", self.ty))
            }
            _ => Ok(Kind::Shape),
        }
    }
}

impl Main {
    /// The built-in defaults for a shape category (`text`/`image`/`shape`).
    pub fn defaults_map(&self, key: &str) -> toml::Table {
        let map = match key {
            "text" => &self.defaults.text,
            "image" => &self.defaults.image,
            _ => &self.defaults.shape,
        };
        let mut out = toml::Table::new();
        for (k, v) in map {
            out.insert(k.clone(), v.clone());
        }
        out
    }
}

impl Project {
    /// Load `main.toml` from a deck directory.
    pub fn load(dir: impl Into<PathBuf>) -> Result<Self> {
        let dir = dir.into();
        let path = dir.join("main.toml");
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| miette!("cannot read `{}`: {e}", path.display()))?;
        let main =
            toml::from_str(&raw).map_err(|e| miette!("cannot parse `{}`: {e}", path.display()))?;
        Ok(Project { dir, main })
    }

    pub fn master_path(&self, stem: &str) -> PathBuf {
        self.dir.join("masters").join(format!("{stem}.toml"))
    }

    pub fn slide_path(&self, file: &str) -> PathBuf {
        self.dir.join("slides").join(file)
    }
}

impl Default for Main {
    fn default() -> Self {
        Main {
            presentation: Presentation::default(),
            layout: Layout {
                name: default_layout_name(),
                width: default_width(),
                height: default_height(),
            },
            theme: default_theme(),
            sections: Vec::new(),
            defaults: Defaults::default(),
            styles: BTreeMap::new(),
        }
    }
}

impl Default for Layout {
    fn default() -> Self {
        Layout {
            name: default_layout_name(),
            width: default_width(),
            height: default_height(),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        default_theme()
    }
}

/// If `sections` is empty, default to a single section covering every slide
/// file in `slides/`.
pub fn default_section(dir: &Path) -> Result<Section> {
    let slides_dir = dir.join("slides");
    let mut names: Vec<String> = Vec::new();
    if slides_dir.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(&slides_dir)
            .map_err(|e| miette!("cannot list `{}`: {e}", slides_dir.display()))?
            .filter_map(|e| e.ok())
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            if entry.file_type().map(|t| t.is_file()).unwrap_or(false)
                && entry.file_name().to_string_lossy().ends_with(".toml")
            {
                names.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }
    if names.is_empty() && !slides_dir.is_dir() {
        return Err(miette!("no `slides/` directory in `{}`", dir.display()));
    }
    Ok(Section {
        title: "Deck".into(),
        slides: names,
    })
}
