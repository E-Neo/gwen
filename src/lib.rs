//! gwen — a Markdown to PowerPoint generator.
//!
//! The source of truth is `config.toml` (layouts + theme), `src/SUMMARY.md`
//! (slide order) and `src/slides/*.md` (content). `build` translates them into
//! a spec and renders the deck with the real pptxgenjs bundle running on an
//! embedded QuickJS runtime, so the output is pptxgenjs's output.

pub mod config;
pub mod content;
pub mod error;
pub mod jsbridge;
pub mod render;
pub mod zip;

use std::path::Path;

use crate::error::Result;

/// Compile and write `target/<name>.pptx`.
pub fn build(project: &Path) -> Result<std::path::PathBuf> {
    let config = config::Config::load(project)?;
    let slides = content::load(project, &config)?;
    let spec = render::spec(project, &config, &slides)?;
    let bytes = jsbridge::render(&spec.to_string())?;

    let out_dir = project.join("target");
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| miette::miette!("cannot create {}: {e}", out_dir.display()))?;
    let out = out_dir.join(format!("{}.pptx", config.presentation.name));
    std::fs::write(&out, bytes)
        .map_err(|e| miette::miette!("cannot write {}: {e}", out.display()))?;
    Ok(out)
}
