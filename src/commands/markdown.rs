use std::io::Write;
use std::path::Path;

use crate::error::AppResult;
use crate::model::presentation::Presentation;
use crate::opc::Package;
use crate::path::ResolvedPath;

/// Dump the whole presentation as an editable Markdown mirror on stdout.
/// With `media_dir`, every image referenced by the deck is written there.
pub fn execute(input: &str, media_dir: Option<&str>) -> AppResult<()> {
    let pkg = Package::open(Path::new(input))?;
    let pres: Presentation = crate::commands::query::load_presentation(&pkg)?;
    let resolved = ResolvedPath::Presentation {
        remaining: Vec::new(),
    };
    let value = crate::commands::query::query_value(&pkg, &pres, &resolved, media_dir)?;
    let md = crate::md::serialize::serialize(&value);
    // Swallow broken-pipe so `markdown ... | head` exits quietly.
    let _ = writeln!(std::io::stdout(), "{md}");
    Ok(())
}
