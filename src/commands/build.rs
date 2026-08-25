use std::path::Path;

use gwen_pptx::engine::{build, validate};
use gwen_pptx::opc::Package;

use crate::diag::plain;

fn presentation_name(dir: &Path) -> miette::Result<String> {
    let cfg = dir.join("config.toml");
    let raw = std::fs::read_to_string(&cfg).map_err(plain)?;
    let value: toml::Value = toml::from_str(&raw).map_err(plain)?;
    let name = value
        .get("presentation")
        .and_then(|p| p.get("name"))
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|n| !n.is_empty());
    match name {
        Some(n) => Ok(n.to_string()),
        None => Err(miette::miette!(
            "`config.toml` must define `[presentation] name`"
        )),
    }
}

/// Compile the project directory into `target/<name>.pptx`. The compiled
/// package is validated before it is written: a structurally invalid deck
/// never reaches disk.
pub fn execute(project: Option<&str>) -> miette::Result<()> {
    let dir = Path::new(project.unwrap_or("."));
    if !dir.join("src").join("PRESENTATION.md").exists() {
        return Err(miette::miette!(
            "`{}` is not a gwen project (no src/PRESENTATION.md)",
            dir.display()
        ));
    }

    let doc = gwen_md::read_document(dir).map_err(plain)?;
    let project = build::Project {
        doc: &doc,
        media_dir: Some(&dir.join("src").join("media")),
    };
    let pkg: Package = build::compile_package(&project).map_err(plain)?;

    validate_or_report(&pkg)?;

    let name = presentation_name(dir)?;
    let out_dir = dir.join("target");
    std::fs::create_dir_all(&out_dir).map_err(plain)?;
    let out = out_dir.join(format!("{name}.pptx"));
    pkg.save(&out).map_err(plain)?;
    eprintln!("built {}", out.display());
    Ok(())
}

/// Run the structural validator; report every violation and refuse to write
/// the output when any is found.
fn validate_or_report(pkg: &Package) -> miette::Result<()> {
    let violations = validate::validate_package(pkg);
    if violations.is_empty() {
        return Ok(());
    }
    for v in &violations {
        eprintln!("  × {}: {}", v.part, v.message);
    }
    Err(miette::miette!(
        "{} invalid package part{} (the deck was not written)",
        violations.len(),
        if violations.len() == 1 { "" } else { "s" }
    ))
}
