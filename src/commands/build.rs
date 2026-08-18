use std::path::Path;

use gwen_md::normalize;
use gwen_pptx::engine::{apply, query, readonly, update_diff};
use gwen_pptx::opc::Package;
use gwen_pptx::path::PathSegment;
use miette::Report;

use crate::diag::{Diag, advice, describe_path, plain};

fn edit_path(path: &[PathSegment]) -> String {
    let mut out = String::new();
    for (i, seg) in path.iter().enumerate() {
        match seg {
            PathSegment::Field(f) => {
                if i > 0 {
                    out.push('.');
                }
                out.push_str(f);
            }
            PathSegment::Index(idx) => out.push_str(&format!("[{idx}]")),
        }
    }
    out
}

/// Whether an edit path falls inside the span key (a dot-separated prefix
/// boundary), so `slides[0].shapes[1]` never matches `slides[0].shapes[10]`.
fn span_matches(key: &str, path: &str) -> bool {
    if !path.starts_with(key) {
        return false;
    }
    path[key.len()..].chars().next().is_none_or(|c| c == '.')
}

pub fn execute(input: &str, md_path: &str, output: &str) -> miette::Result<()> {
    let mut pkg = Package::open(Path::new(input)).map_err(plain)?;
    let pres = query::load_presentation(&pkg).map_err(plain)?;

    let md = std::fs::read_to_string(md_path).map_err(plain)?;
    let parsed =
        gwen_md::parse(&md).map_err(|e| Report::new(Diag::at(&md, e.message, &e.span, None)))?;
    let new = normalize::normalize(&parsed.doc);
    let current = query::query_document(&pkg, None).map_err(plain)?;
    let edits = update_diff::diff(&normalize::normalize(&readonly::project(&current)), &new);

    for edit in update_diff::order_edits(edits) {
        if let Err(e) = apply::apply_edit(&mut pkg, &pres, &edit) {
            let path_str = edit_path(&edit.path);
            let span = parsed
                .spans
                .iter()
                .filter(|(k, _)| span_matches(k, &path_str))
                .max_by_key(|(k, _)| k.len())
                .map(|(_, s)| s);
            let mut help = Vec::new();
            let loc = describe_path(&parsed.doc, &path_str);
            if !loc.is_empty() {
                help.push(format!("at {loc}"));
            }
            if let Some(suggestion) = advice(&e.to_string()) {
                help.push(suggestion);
            }
            let help = (!help.is_empty()).then(|| help.join("\n"));
            return match span {
                Some(s) => Err(Report::new(Diag::at(&md, e.to_string(), s, help))),
                None => Err(Report::new(Diag::plain(match help {
                    Some(h) => format!("{e} at '{path_str}' ({h})"),
                    None => format!("{e} at '{path_str}'"),
                }))),
            };
        }
    }

    pkg.save(Path::new(output)).map_err(plain)?;
    Ok(())
}
