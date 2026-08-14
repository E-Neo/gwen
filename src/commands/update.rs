use std::path::Path;

use gwen_md::parse;
use gwen_pptx::engine::{apply, query, readonly, update_diff};
use gwen_pptx::error::AppResult;
use gwen_pptx::opc::Package;

pub fn execute(input: &str, md_path: &str, output: &str) -> AppResult<()> {
    let mut pkg = Package::open(Path::new(input))?;
    let pres = query::load_presentation(&pkg)?;

    let md = std::fs::read_to_string(md_path)?;
    let new = parse::parse(&md);
    let current = query::query_document(&pkg, None)?;
    let edits = update_diff::diff(&readonly::project(&current), &new);

    apply::apply_edits(&mut pkg, &pres, edits)?;

    pkg.save(Path::new(output))?;
    Ok(())
}
