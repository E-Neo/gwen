//! gwen — a TOML to PowerPoint generator.
//!
//! A deck is a directory with `main.toml` (presentation/layout/theme, the
//! `[[sections]]` slide index, `[defaults.*]` and `[styles.*]`), a
//! `masters/*.toml` and `slides/*.toml`. `build` translates them into a spec
//! and renders the deck with the real pptxgenjs bundle running on an embedded
//! QuickJS runtime, so the output is pptxgenjs's output — gwen never writes
//! OOXML itself.

pub mod error;
pub mod jsbridge;
pub mod model;
pub mod render;
pub mod richtext;
pub mod units;
pub mod zip;

use std::path::Path;

use miette::miette;

use crate::error::Result;

/// Compile and write `target/<name>.pptx`.
pub fn build(project_dir: &Path) -> Result<std::path::PathBuf> {
    let project = model::Project::load(project_dir)?;
    let spec = render::spec_json(&project)?;
    let bytes = jsbridge::render(&spec)?;

    let out_dir = project_dir.join("target");
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| miette!("cannot create {}: {e}", out_dir.display()))?;
    let title = project.main.presentation.title.trim();
    let name = if title.is_empty() { "deck" } else { title };
    let out = out_dir.join(format!("{name}.pptx"));
    std::fs::write(&out, bytes).map_err(|e| miette!("cannot write {}: {e}", out.display()))?;
    Ok(out)
}
