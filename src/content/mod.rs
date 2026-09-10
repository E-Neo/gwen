//! Slide content: `SUMMARY.md` ordering plus per-slide Markdown parsing.

pub mod slide;
pub mod summary;

use std::path::Path;

use crate::config::Config;
use crate::error::Result;

/// One paragraph of slide body text.
#[derive(Debug, Clone, PartialEq)]
pub struct Paragraph {
    pub text: String,
    /// Bullet nesting level (0 = top level).
    pub level: i32,
    pub bullet: bool,
}

/// A parsed slide.
#[derive(Debug, Clone, PartialEq)]
pub struct Slide {
    /// Path of the source file, relative to the project (for diagnostics).
    pub source: String,
    pub layout: String,
    pub background: Option<String>,
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub body: Vec<Paragraph>,
    /// Media filenames (relative to `src/media`) referenced by the slide.
    pub pictures: Vec<String>,
    pub notes: Option<String>,
}

/// Load every slide listed in `src/SUMMARY.md`, in order.
pub fn load(project: &Path, config: &Config) -> Result<Vec<Slide>> {
    let summary_path = project.join("src").join("SUMMARY.md");
    let summary = std::fs::read_to_string(&summary_path)
        .map_err(|e| miette::miette!("cannot read {}: {e}", summary_path.display()))?;
    let entries = summary::parse(&summary);

    let mut slides = Vec::new();
    for rel in entries {
        let path = project.join("src").join(&rel);
        let text = std::fs::read_to_string(&path)
            .map_err(|e| miette::miette!("SUMMARY.md lists `{rel}`, which cannot be read: {e}"))?;
        let slide = slide::parse(&rel, &text)?;
        // Resolve the layout here so the diagnostic names the slide file.
        config
            .layout(&slide.layout)
            .map_err(|e| miette::miette!("slide `{rel}`: {e}"))?;
        slides.push(slide);
    }
    if slides.is_empty() {
        return Err(miette::miette!("`src/SUMMARY.md` lists no slides"));
    }
    Ok(slides)
}
