//! `gwen render`: paint slides straight to SVG/PNG without an office suite.
//!
//! The pipeline is spec -> resolved scene (`scene`) -> laid out text
//! (`textlayout`) -> SVG (`svg`) -> PNG (resvg). Output is named after the
//! source slide TOML: `slides/01-trend.toml` becomes
//! `<deck>/target/render/01-trend.{svg,png}`. PNG is written by default and
//! is the artifact a vision model inspects; SVG is only written when
//! requested. `--html` additionally writes `target/render/index.html`, a
//! self-contained slideshow that embeds the first requested format. Every
//! still-approximated feature is reported on stderr.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use base64::Engine;
use miette::miette;

use crate::defaults;
use crate::error::Result;
use crate::fonts::Fonts;
use crate::model::Project;
use crate::pages::PageSelection;
use crate::render;
use crate::scene::{self, Fit, ItemKind, Scene};

/// One image format understood by `gwen render --format`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Svg,
}

impl ImageFormat {
    fn mime(self) -> &'static str {
        match self {
            ImageFormat::Png => "image/png",
            ImageFormat::Svg => "image/svg+xml",
        }
    }
}

/// The output formats a `gwen render --format` call asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Formats {
    pub png: bool,
    pub svg: bool,
    /// The first requested format; `--html` embeds this one.
    pub primary: ImageFormat,
}

impl Formats {
    /// Parse a comma-separated list (`png`, `png,svg`). The first entry is
    /// the primary format used by `--html`.
    pub fn parse(input: &str) -> Result<Self> {
        let mut png = false;
        let mut svg = false;
        let mut primary = None;
        for raw in input.split(',') {
            let format = match raw.trim().to_ascii_lowercase().as_str() {
                "png" => {
                    png = true;
                    ImageFormat::Png
                }
                "svg" => {
                    svg = true;
                    ImageFormat::Svg
                }
                other => {
                    return Err(miette!(
                        "unknown render format `{other}`; use `png` and/or `svg`"
                    ));
                }
            };
            primary.get_or_insert(format);
        }
        let Some(primary) = primary else {
            return Err(miette!(
                "`--format` selects nothing; use `png` and/or `svg`"
            ));
        };
        Ok(Formats { png, svg, primary })
    }
}

/// Render the selected slides and return every file written. With `html`,
/// also write `target/render/index.html` embedding the primary format.
pub fn render(
    project_dir: &Path,
    pages: Option<&PageSelection>,
    formats: Formats,
    html: bool,
) -> Result<Vec<PathBuf>> {
    let project = Project::load(project_dir)?;
    let title = project.main.presentation.title.trim();
    let title = if title.is_empty() { "deck" } else { title };
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
    let mut embedded = Vec::new();
    for page in &selected {
        let mut scene = scenes[page - 1].clone();
        apply_autofit(&mut scene, &fonts);
        collect_layout_approximations(&mut scene, &fonts);
        let svg_text = crate::svg::render(&scene, &fonts);
        let stem = stems
            .get(page)
            .expect("every selected slide has a validated output stem");

        let needs_png = formats.png || (html && formats.primary == ImageFormat::Png);
        let png_bytes = if needs_png {
            Some(rasterize_png(&svg_text, &fonts)?)
        } else {
            None
        };

        if formats.svg {
            let path = target.join(format!("{stem}.svg"));
            std::fs::write(&path, svg_text.as_bytes())
                .map_err(|e| miette!("cannot write `{}`: {e}", path.display()))?;
            written.push(path);
        }
        if formats.png {
            let path = target.join(format!("{stem}.png"));
            let bytes = png_bytes
                .as_deref()
                .expect("PNG bytes are encoded when the PNG format is requested");
            std::fs::write(&path, bytes)
                .map_err(|e| miette!("cannot write `{}`: {e}", path.display()))?;
            written.push(path);
        }
        if html {
            let bytes = match formats.primary {
                ImageFormat::Png => png_bytes
                    .as_deref()
                    .expect("PNG bytes are encoded when the primary format is PNG"),
                ImageFormat::Svg => svg_text.as_bytes(),
            };
            embedded.push((stem.clone(), data_uri(formats.primary, bytes)));
        }

        for message in &scene.approximations {
            eprintln!("gwen: slide {page}: {message}");
        }
    }

    if html {
        let path = target.join("index.html");
        let document = slideshow_html(title, &embedded);
        std::fs::write(&path, document.as_bytes())
            .map_err(|e| miette!("cannot write `{}`: {e}", path.display()))?;
        written.push(path);
    }
    Ok(written)
}

/// Base64 data URI for the embedded image.
fn data_uri(format: ImageFormat, bytes: &[u8]) -> String {
    format!(
        "data:{};base64,{}",
        format.mime(),
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

/// A self-contained slideshow: every slide is a data URI, so the document
/// works from `file://` and when moved on its own.
fn slideshow_html(title: &str, slides: &[(String, String)]) -> String {
    let title = escape_html(title);
    let mut out = String::new();
    out.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n");
    out.push_str("<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    out.push_str("<title>");
    out.push_str(&title);
    out.push_str("</title>\n<style>\n");
    out.push_str(SLIDESHOW_CSS);
    out.push_str("</style>\n</head>\n<body>\n<header><h1>");
    out.push_str(&title);
    out.push_str("</h1><p>Press <kbd>f</kbd> to present</p></header>\n<main>\n");
    for (stem, data) in slides {
        let name = escape_html(stem);
        out.push_str("<figure class=\"slide\"><figcaption>");
        out.push_str(&name);
        out.push_str("</figcaption><img src=\"");
        out.push_str(data);
        out.push_str("\" alt=\"");
        out.push_str(&name);
        out.push_str("\"></figure>\n");
    }
    out.push_str("</main>\n<script>\n");
    out.push_str(SLIDESHOW_JS);
    out.push_str("</script>\n</body>\n</html>\n");
    out
}

fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

const SLIDESHOW_CSS: &str = r#"
* { box-sizing: border-box; }
html, body { margin: 0; }
body { background: #1b1b1f; color: #f5f5f7; font: 14px/1.4 system-ui, sans-serif; }
header { display: flex; align-items: baseline; justify-content: space-between; gap: 1rem; padding: 1rem 1.5rem; }
header h1 { margin: 0; font-size: 1rem; font-weight: 600; }
header p { margin: 0; opacity: .65; }
kbd { padding: 0 .35em; border: 1px solid #666; border-radius: 4px; font: inherit; }
main { display: flex; flex-direction: column; gap: 1.5rem; padding: 0 1.5rem 2rem; }
.slide { margin: 0; }
.slide figcaption { margin: 0 0 .45rem; color: #b9b9c2; font: 12px/1.3 ui-monospace, monospace; }
.slide img { display: block; width: 100%; height: auto; border-radius: 4px; background: #fff; }
body.presenting { overflow: hidden; background: #000; }
body.presenting header { display: none; }
body.presenting main { display: block; padding: 0; }
body.presenting .slide { display: none; }
body.presenting .slide figcaption { display: none; }
body.presenting .slide.active { position: fixed; inset: 0; display: flex; align-items: center; justify-content: center; }
body.presenting .slide img { width: 100vw; height: 100vh; object-fit: contain; border-radius: 0; background: #000; }
"#;

const SLIDESHOW_JS: &str = r#"
(() => {
  const slides = Array.from(document.querySelectorAll('.slide'));
  let current = 0;
  let presenting = false;

  const show = (index) => {
    current = Math.max(0, Math.min(slides.length - 1, index));
    slides.forEach((slide, i) => slide.classList.toggle('active', i === current));
  };

  const begin = () => {
    if (presenting) return;
    presenting = true;
    document.body.classList.add('presenting');
    show(0);
    if (document.documentElement.requestFullscreen) {
      document.documentElement.requestFullscreen().catch(() => {});
    }
  };

  const end = () => {
    if (!presenting) return;
    presenting = false;
    document.body.classList.remove('presenting');
    slides.forEach((slide) => slide.classList.remove('active'));
    if (document.fullscreenElement && document.exitFullscreen) {
      document.exitFullscreen().catch(() => {});
    }
  };

  document.addEventListener('keydown', (event) => {
    const key = event.key;
    if (key === 'f' || key === 'F') {
      event.preventDefault();
      if (presenting) { end(); } else { begin(); }
      return;
    }
    if (!presenting) return;
    if (['ArrowRight', 'ArrowDown', 'PageDown', ' ', 'Enter'].includes(key)) {
      event.preventDefault();
      show(current + 1);
    } else if (['ArrowLeft', 'ArrowUp', 'PageUp'].includes(key)) {
      event.preventDefault();
      show(current - 1);
    } else if (key === 'Home') {
      event.preventDefault();
      show(0);
    } else if (key === 'End') {
      event.preventDefault();
      show(slides.length - 1);
    } else if (key === 'Escape') {
      end();
    }
  });

  document.addEventListener('click', () => {
    if (presenting) show(current + 1);
  });

  document.addEventListener('fullscreenchange', () => {
    if (document.fullscreenElement && !presenting) {
      begin();
    } else if (!document.fullscreenElement && presenting) {
      presenting = false;
      document.body.classList.remove('presenting');
      slides.forEach((slide) => slide.classList.remove('active'));
    }
  });
})();
"#;

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

/// Remove rendered images and the slideshow from an earlier invocation so a
/// `--format png` run cannot leave a stale `.svg` (or a previous page
/// selection) behind. Also drops the `preview.json` written by older gwen
/// versions.
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
        let file_name = path.file_name().and_then(|name| name.to_str());
        let is_legacy_report = file_name == Some("preview.json");
        let is_slideshow = file_name == Some("index.html");
        if is_image || is_legacy_report || is_slideshow {
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
            let mut block = block.clone();
            crate::textlayout::normalize_fonts(&mut block, fonts);
            let layout = crate::textlayout::layout(&block, item.rect.w, item.rect.h, fonts);
            messages.extend(layout.approximations);
        }
    }
    for message in messages {
        if !scene.approximations.contains(&message) {
            scene.approximations.push(message);
        }
    }
}

fn rasterize_png(svg: &str, fonts: &Fonts) -> Result<Vec<u8>> {
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
        .encode_png()
        .map_err(|e| miette!("cannot encode the preview PNG: {e}"))
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

    #[test]
    fn formats_keep_the_first_requested_format() {
        assert_eq!(Formats::parse("png").unwrap().primary, ImageFormat::Png);
        assert_eq!(Formats::parse("svg").unwrap().primary, ImageFormat::Svg);
        assert_eq!(Formats::parse("svg,png").unwrap().primary, ImageFormat::Svg);
        assert_eq!(Formats::parse("png,svg").unwrap().primary, ImageFormat::Png);
        assert_eq!(
            Formats::parse(" PNG , svg ").unwrap().primary,
            ImageFormat::Png
        );
    }

    #[test]
    fn slideshow_is_self_contained() {
        let document = slideshow_html(
            "A & B",
            &[("one".to_string(), "data:image/png;base64,AAAA".to_string())],
        );
        assert!(document.starts_with("<!doctype html>"));
        assert!(document.contains("<title>A &amp; B</title>"));
        assert!(document.contains("src=\"data:image/png;base64,AAAA\""));
        assert!(document.contains("alt=\"one\""));
        assert!(document.contains("requestFullscreen"));
        assert!(!document.contains("<link"));
        assert!(!document.contains("<script src"));
    }
}
