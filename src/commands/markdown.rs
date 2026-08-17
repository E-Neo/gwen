use std::io::Write;
use std::path::Path;

use gwen_md::serialize;
use gwen_pptx::engine::query;
use gwen_pptx::opc::Package;

use crate::diag::plain;

/// Dump the whole presentation as an editable Markdown mirror on stdout.
/// With `media_dir`, every image referenced by the deck is written there.
pub fn execute(input: &str, media_dir: Option<&str>) -> miette::Result<()> {
    let pkg = Package::open(Path::new(input)).map_err(plain)?;
    let value = query::query_document(&pkg, media_dir).map_err(plain)?;
    let md = serialize::serialize(&value);
    // Swallow broken-pipe so `markdown ... | head` exits quietly.
    let _ = writeln!(std::io::stdout(), "{md}");
    Ok(())
}
