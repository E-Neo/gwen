//! `gwen render`: paint slides straight to SVG/PNG without an office suite.
//!
//! The pipeline is spec -> resolved scene (`scene`) -> laid out text
//! (`textlayout`) -> SVG (`svg`) -> PNG (resvg). Output is named after the
//! source slide TOML: `slides/01-trend.toml` becomes
//! `<deck>/target/render/01-trend.{svg,png}`. PNG is written by default and
//! is the artifact a vision model inspects; SVG is only written when
//! requested. Every still-approximated feature is reported on stderr.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use miette::miette;

use crate::defaults;
use crate::error::Result;
use crate::fonts::Fonts;
use crate::model::Project;
use crate::pages::PageSelection;
use crate::render;
use crate::scene::{self, Fit, ItemKind, Scene};

/// The output formats a `gwen render --format` call asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Formats {
    pub png: bool,
    pub svg: bool,
}

impl Formats {
    /// Parse a comma-separated list (`png`, `png,svg`).
    pub fn parse(input: &str) -> Result<Self> {
        let mut formats = Formats {
            png: false,
            svg: false,
        };
        for raw in input.split(',') {
            match raw.trim().to_ascii_lowercase().as_str() {
                "png" => formats.png = true,
                "svg" => formats.svg = true,
                other => {
                    return Err(miette!(
                        "unknown render format `{other}`; use `png` and/or `svg`"
                    ));
                }
            }
        }
        if !formats.png && !formats.svg {
            return Err(miette!(
                "`--format` selects nothing; use `png` and/or `svg`"
            ));
        }
        Ok(formats)
    }
}

/// Render the selected slides and return every file written.
pub fn render(
    project_dir: &Path,
    pages: Option<&PageSelection>,
    formats: Formats,
) -> Result<Vec<PathBuf>> {
    let project = Project::load(project_dir)?;
    let spec = render::spec_json(&project)?;
    let spec: serde_json::Value = serde_json::from_str(&spec)
        .map_err(|e| miette!("the spec pptxgenjs built is invalid JSON: {e}"))?;
    let fonts = Fonts::load(project_dir);
    let defaults = defaults::get(
        &project.main.theme.major_font,
        &project.main.theme.minor_font,
    )?;
    let scenes = scene::build(&spec, &defaults, project_dir).map_err(|e| miette!("{e}"))?;

    let mut selected: BTreeSet<usize> = BTreeSet::new();
    match pages {
        None => selected.extend(1..=scenes.len()),
        Some(selection) => {
            for page in selection.select(scenes.len()).map_err(|e| miette!("{e}"))? {
                selected.insert(page);
            }
        }
    }

    let mut stems = BTreeMap::new();
    let mut owners = BTreeMap::new();
    for page in &selected {
        let stem = slide_stem(&scenes[page - 1])?;
        if let Some(&other) = owners.get(&stem) {
            return Err(miette!(
                "slides {other} and {page} both render to `{stem}.png`; rename one of their source files"
            ));
        }
        owners.insert(stem.clone(), *page);
        stems.insert(*page, stem);
    }

    let target = project_dir.join("target").join("render");
    std::fs::create_dir_all(&target)
        .map_err(|e| miette!("cannot create `{}`: {e}", target.display()))?;
    clear_previous(&target)?;

    let mut written = Vec::new();
    for page in &selected {
        let mut scene = scenes[page - 1].clone();
        apply_autofit(&mut scene, &fonts);
        collect_layout_approximations(&mut scene, &fonts);
        let svg_text = crate::svg::render(&scene, &fonts);
        let stem = stems
            .get(page)
            .expect("every selected slide has a validated output stem");

        if formats.svg {
            let path = target.join(format!("{stem}.svg"));
            std::fs::write(&path, svg_text.as_bytes())
                .map_err(|e| miette!("cannot write `{}`: {e}", path.display()))?;
            written.push(path);
        }
        if formats.png {
            let path = target.join(format!("{stem}.png"));
            rasterize(&svg_text, &path, &fonts)?;
            written.push(path);
        }

        for message in &scene.approximations {
            eprintln!("gwen: slide {page}: {message}");
        }
    }
    Ok(written)
}

/// The output stem is the source slide's file stem, matching the TOML name:
/// `slides/01-trend.toml` -> `01-trend`.
fn slide_stem(scene: &Scene) -> Result<String> {
    Path::new(&scene.file)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            miette!(
                "slide {} has no source file name; cannot name its preview",
                scene.page
            )
        })
}

/// Remove rendered images from an earlier invocation so a `--format png` run
/// cannot leave a stale `.svg` (or a previous page selection) behind. Also
/// drops the `preview.json` written by older gwen versions.
fn clear_previous(dir: &Path) -> Result<()> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| miette!("cannot list `{}`: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| miette!("cannot list `{}`: {e}", dir.display()))?;
        let path = entry.path();
        let is_image = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("png") || ext.eq_ignore_ascii_case("svg"));
        let is_legacy_report =
            path.file_name().and_then(|name| name.to_str()) == Some("preview.json");
        if is_image || is_legacy_report {
            std::fs::remove_file(&path)
                .map_err(|e| miette!("cannot remove `{}`: {e}", path.display()))?;
        }
    }
    Ok(())
}

/// `fit: resize` (spAutoFit) sizes the text box to its content. Wrapping text
/// keeps its width and adjusts height; unwrapped and vertical text can adjust
/// both axes. A shape-preset text box resizes its backing shape too.
fn apply_autofit(scene: &mut Scene, fonts: &Fonts) {
    for index in 0..scene.items.len() {
        let grow = match &scene.items[index].kind {
            ItemKind::Text(block) if block.fit == Fit::Resize => {
                let layout = crate::textlayout::layout(
                    block,
                    scene.items[index].rect.w,
                    scene.items[index].rect.h,
                    fonts,
                );
                layout.resize
            }
            _ => None,
        };
        if let Some((new_w, new_h)) = grow {
            let old = scene.items[index].rect;
            scene.items[index].rect.w = new_w;
            scene.items[index].rect.h = new_h;
            if let ItemKind::Text(block) = &mut scene.items[index].kind {
                block.valign = crate::scene::Valign::Top;
            }
            if index > 0 {
                let shape = &mut scene.items[index - 1];
                if matches!(shape.kind, ItemKind::Shape { .. })
                    && (shape.rect.x - old.x).abs() < 1e-9
                    && (shape.rect.y - old.y).abs() < 1e-9
                    && (shape.rect.w - old.w).abs() < 1e-9
                {
                    shape.rect.w = new_w;
                    shape.rect.h = new_h;
                }
            }
        }
    }
}

/// Text layout is recomputed by the painter, so collect its warnings on the
/// same final boxes before reporting them with the scene's approximations.
fn collect_layout_approximations(scene: &mut Scene, fonts: &Fonts) {
    let mut messages = Vec::new();
    for item in &scene.items {
        if let ItemKind::Text(block) = &item.kind {
            let layout = crate::textlayout::layout(block, item.rect.w, item.rect.h, fonts);
            messages.extend(layout.approximations);
        }
    }
    for message in messages {
        if !scene.approximations.contains(&message) {
            scene.approximations.push(message);
        }
    }
}

fn rasterize(svg: &str, path: &Path, fonts: &Fonts) -> Result<()> {
    let options = usvg::Options {
        fontdb: fonts.database(),
        ..Default::default()
    };
    let tree = usvg::Tree::from_str(svg, &options)
        .map_err(|e| miette!("cannot parse the generated SVG: {e}"))?;
    let size = tree.size();
    // Raster at 2x (192 dpi for a 96 dpi base) so the vision model reads
    // crisp text.
    const SCALE: f32 = 2.0;
    let width = (size.width() * SCALE).round().max(1.0) as u32;
    let height = (size.height() * SCALE).round().max(1.0) as u32;
    let mut pixmap = tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| miette!("cannot allocate a {width}x{height} bitmap"))?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(SCALE, SCALE),
        &mut pixmap.as_mut(),
    );
    pixmap
        .save_png(path)
        .map_err(|e| miette!("cannot write `{}`: {e}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_parse_lists() {
        let formats = Formats::parse("png").unwrap();
        assert!(formats.png && !formats.svg);
        let formats = Formats::parse("png,svg").unwrap();
        assert!(formats.png && formats.svg);
        let formats = Formats::parse("SVG,PNG").unwrap();
        assert!(formats.png && formats.svg);
        assert!(Formats::parse("jpg").is_err());
        assert!(Formats::parse("").is_err());
    }
}
