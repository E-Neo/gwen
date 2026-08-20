use std::path::Path;

use gwen_pptx::engine::query;
use gwen_pptx::opc::Package;

use crate::diag::plain;

fn write(data: &[u8], path: &Path) -> miette::Result<()> {
    std::fs::create_dir_all(path.parent().expect("path has a parent")).map_err(plain)?;
    std::fs::write(path, data).map_err(plain)
}

/// Create a new project directory from a template `.pptx`. The markdown mirror
/// plus the extracted media are the project's only source of truth; the
/// template's structural XML is regenerated from standard Office defaults on
/// build.
pub fn execute(project: &str, template: &str) -> miette::Result<()> {
    let dir = Path::new(project);
    if dir.exists() {
        return Err(miette::miette!("project `{project}` already exists"));
    }

    let pkg = Package::open(Path::new(template)).map_err(plain)?;
    let media_dir = dir.join("src").join("media");
    std::fs::create_dir_all(&media_dir).map_err(plain)?;

    let snapshot = query::query_document(&pkg, media_dir.to_str()).map_err(plain)?;
    gwen_md::serialize::write_document(&snapshot, dir).map_err(plain)?;

    let name = Path::new(template)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("deck");
    let config = format!("[presentation]\nname = \"{name}\"\n");
    write(config.as_bytes(), &dir.join("config.toml"))?;

    eprintln!("created project `{project}` from `{template}`");
    eprintln!(
        "  edit {}/src/PRESENTATION.md, then run `gwen build {}`",
        project, project
    );
    Ok(())
}
