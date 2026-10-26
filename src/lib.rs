//! gwen - a TOML to PowerPoint generator.
//!
//! A deck is a directory with `main.toml` (presentation/theme/sections/styles),
//! `masters/*.toml` and `slides/*.toml`. `build` translates them into a spec
//! and renders that spec with gwen's native OOXML/PPTX writer, reproducing
//! pptxgenjs 4.0.1's output format. See the project README for the full format
//! reference.

pub mod color;
pub mod defaults;
pub mod error;
pub mod fonts;
pub mod import;
pub mod media;
pub mod model;
pub mod opts;
pub mod pages;
pub mod pptx;
pub mod presets;
pub mod preview;
pub mod render;
pub mod richtext;
pub mod scene;
pub mod svg;
pub mod textlayout;
pub mod units;
pub mod xml;
pub mod zip;

pub use crate::import::import_pptx;

use std::path::Path;

use miette::miette;

use crate::error::Result;

/// Compile and write `target/<name>.pptx`.
pub fn build(project_dir: &Path) -> Result<std::path::PathBuf> {
    let project = model::Project::load(project_dir)?;
    let spec = render::spec_json(&project)?;
    let bytes = pptx::render(&spec)?;

    let out_dir = project_dir.join("target");
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| miette!("cannot create {}: {e}", out_dir.display()))?;
    let title = project.main.presentation.title.trim();
    let name = if title.is_empty() { "deck" } else { title };
    let out = out_dir.join(format!("{name}.pptx"));
    std::fs::write(&out, bytes).map_err(|e| miette!("cannot write {}: {e}", out.display()))?;
    Ok(out)
}

/// Remove the generated `target/` directory, returning whether it existed.
///
/// Only directories that contain `main.toml` are touched, so cleaning a
/// non-gwen directory cannot delete an unrelated `target/`.
pub fn clean(project_dir: &Path) -> Result<bool> {
    if !project_dir.join("main.toml").is_file() {
        return Err(miette!(
            "`{}` is not a gwen project (no main.toml)",
            project_dir.display()
        ));
    }
    let target = project_dir.join("target");
    match std::fs::symlink_metadata(&target) {
        Ok(meta) if meta.is_dir() => {
            std::fs::remove_dir_all(&target)
                .map_err(|e| miette!("cannot remove `{}`: {e}", target.display()))?;
            Ok(true)
        }
        Ok(_) => {
            // A file or symlink at target/ is removed as-is, never followed.
            std::fs::remove_file(&target)
                .map_err(|e| miette!("cannot remove `{}`: {e}", target.display()))?;
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(miette!("cannot inspect `{}`: {e}", target.display())),
    }
}
