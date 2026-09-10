//! gwen — a Markdown to PowerPoint generator.
//!
//! The source of truth is `config.toml` (layouts + theme), `src/SUMMARY.md`
//! (slide order) and `src/slides/*.md` (content). `build` compiles them into a
//! clean, structurally valid `.pptx`. There is no round-trip: a template only
//! ever seeds a new project.

pub mod config;
pub mod content;
pub mod engine;
pub mod error;
pub mod ooxml;
pub mod parts;
pub mod validate;

use std::path::Path;

use crate::error::Result;

/// Compile a project directory into a package and return it.
pub fn compile(project: &Path) -> Result<ooxml::package::Package> {
    let config = config::Config::load(project)?;
    let slides = content::load(project, &config)?;
    engine::build(project, &config, &slides)
}

/// Compile and write `target/<name>.pptx`, refusing to write an invalid
/// package.
pub fn build(project: &Path) -> Result<std::path::PathBuf> {
    let config = config::Config::load(project)?;
    let slides = content::load(project, &config)?;
    let pkg = engine::build(project, &config, &slides)?;

    let violations = validate::validate(&pkg);
    if !violations.is_empty() {
        for v in &violations {
            eprintln!("  × {}: {}", v.part, v.message);
        }
        return Err(miette::miette!(
            "{} invalid package part{} (nothing written)",
            violations.len(),
            if violations.len() == 1 { "" } else { "s" }
        ));
    }

    let out_dir = project.join("target");
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| miette::miette!("cannot create {}: {e}", out_dir.display()))?;
    let out = out_dir.join(format!("{}.pptx", config.presentation.name));
    pkg.save(&out)?;
    Ok(out)
}
