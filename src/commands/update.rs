use std::path::Path;

use gwen_md::normalize;
use gwen_pptx::engine::{apply, query, readonly, update_diff};
use gwen_pptx::error::{AppError, AppResult};
use gwen_pptx::opc::Package;
use gwen_pptx::path::PathSegment;

fn edit_path(path: &[PathSegment]) -> String {
    path.iter()
        .map(|seg| match seg {
            PathSegment::Field(f) => f.clone(),
            PathSegment::Index(i) => format!("[{i}]"),
        })
        .collect()
}

pub fn execute(input: &str, md_path: &str, output: &str) -> AppResult<()> {
    let mut pkg = Package::open(Path::new(input))?;
    let pres = query::load_presentation(&pkg)?;

    let md = std::fs::read_to_string(md_path)?;
    let parsed = gwen_md::parse(&md).map_err(|e| AppError::Markdown(e.render()))?;
    let new = normalize::normalize(&parsed.doc);
    let current = query::query_document(&pkg, None)?;
    let edits = update_diff::diff(&normalize::normalize(&readonly::project(&current)), &new);

    for edit in update_diff::order_edits(edits) {
        if let Err(e) = apply::apply_edit(&mut pkg, &pres, &edit) {
            let path_str = edit_path(&edit.path);
            let span = parsed
                .spans
                .iter()
                .filter(|(k, _)| path_str.starts_with(k.as_str()))
                .max_by_key(|(k, _)| k.len())
                .map(|(_, s)| s);
            let msg = match span {
                Some(span) => format!("{e}\n{}", span.render_at(&path_str)),
                None => format!("{e} at '{path_str}'"),
            };
            return Err(AppError::Markdown(msg));
        }
    }

    pkg.save(Path::new(output))?;
    Ok(())
}
