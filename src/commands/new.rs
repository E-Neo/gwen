use std::path::Path;

use gwen_pptx::engine::query;
use gwen_pptx::opc::Package;

use crate::diag::plain;

/// Find the byte offset of `needle` in `data`, starting the search at `from`.
fn find_sub(data: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    data.get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

/// Byte offset just past the `>` that closes an opening tag starting at `from`.
fn close_gt(data: &[u8], from: usize) -> Option<usize> {
    data.get(from..)?
        .iter()
        .position(|&b| b == b'>')
        .map(|p| p + from + 1)
}

/// Split a `<p:cSld>` part (slide, layout, master, notes slide) into the
/// `.head` (through `<p:cSld ...>`), `.mid` (between cSld and spTree, the
/// background), `.sp` (through `<p:spTree>`), and `.post` (everything after
/// `</p:spTree>`) fragments the compiler reassembles.
fn split_csld(data: &[u8]) -> Option<[Vec<u8>; 4]> {
    let csld = find_sub(data, b"<p:cSld", 0)?;
    let head_end = close_gt(data, csld)?;
    let sp = find_sub(data, b"<p:spTree", head_end)?;
    let sp_end = close_gt(data, sp)?;
    let sp_close = find_sub(data, b"</p:spTree>", sp_end)?;
    let sp_close_end = sp_close + b"</p:spTree>".len();
    Some([
        data[..head_end].to_vec(),
        data[head_end..sp].to_vec(),
        data[sp..sp_end].to_vec(),
        data[sp_close_end..].to_vec(),
    ])
}

/// The unmodeled trailing children of a regenerated part, from the closing of
/// `open` to the opening of `close`.
fn split_tail(data: &[u8], open: &[u8], close: &[u8]) -> Option<Vec<u8>> {
    let a = find_sub(data, open, 0)? + open.len();
    let b = find_sub(data, close, a)?;
    Some(data[a..b].to_vec())
}

fn numbered(uri: &str, prefix: &str) -> bool {
    uri.strip_prefix(prefix)
        .and_then(|r| r.strip_suffix(".xml"))
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

/// Whether the part's XML is split into fragments instead of being copied
/// verbatim.
fn is_csld_uri(uri: &str) -> bool {
    numbered(uri, "ppt/slides/slide")
        || numbered(uri, "ppt/slideLayouts/slideLayout")
        || numbered(uri, "ppt/slideMasters/slideMaster")
        || numbered(uri, "ppt/notesSlides/notesSlide")
}

fn write(data: &[u8], path: &Path) -> miette::Result<()> {
    std::fs::create_dir_all(path.parent().expect("path has a parent")).map_err(plain)?;
    std::fs::write(path, data).map_err(plain)
}

/// Capture the package's preserved parts, rels, fragments and content types
/// into `src/parts`.
fn capture_parts(pkg: &Package, parts_dir: &Path) -> miette::Result<()> {
    for uri in pkg.part_uris() {
        let data = pkg
            .get_part(uri)
            .expect("part_uris and get_part agree")
            .to_vec();
        if uri == "[Content_Types].xml" {
            continue;
        }
        if is_csld_uri(uri) {
            if let Some([head, mid, sp, post]) = split_csld(&data) {
                let frag = |suffix: &str, bytes: &[u8]| {
                    write(
                        bytes,
                        &parts_dir.join("_fragments").join(format!("{uri}{suffix}")),
                    )
                };
                frag(".head", &head)?;
                frag(".mid", &mid)?;
                frag(".sp", &sp)?;
                frag(".post", &post)?;
            }
        } else if uri == "ppt/presentation.xml" {
            if let Some(tail) = split_tail(&data, b"</p:sldIdLst>", b"</p:presentation>") {
                write(
                    &tail,
                    &parts_dir.join("_fragments").join(format!("{uri}.tail")),
                )?;
            }
        } else if uri.starts_with("ppt/theme/") {
            if let Some(tail) = split_tail(&data, b"</a:fontScheme>", b"</a:themeElements>") {
                write(
                    &tail,
                    &parts_dir.join("_fragments").join(format!("{uri}.tail")),
                )?;
            }
        } else {
            write(&data, &parts_dir.join(uri))?;
        }
        if let Some(rels) = pkg.rels_xml_for(uri) {
            write(&rels, &parts_dir.join("_rels").join(format!("{uri}.rels")))?;
        }
    }
    Ok(())
}

/// Create a new project directory from a template `.pptx`.
pub fn execute(project: &str, template: &str) -> miette::Result<()> {
    let dir = Path::new(project);
    if dir.exists() {
        return Err(miette::miette!("project `{project}` already exists"));
    }

    let pkg = Package::open(Path::new(template)).map_err(plain)?;
    let parts_dir = dir.join("src").join("parts");
    let media_dir = dir.join("src").join("media");
    std::fs::create_dir_all(&media_dir).map_err(plain)?;

    let snapshot = query::query_document(&pkg, media_dir.to_str()).map_err(plain)?;
    gwen_md::serialize::write_document(&snapshot, dir).map_err(plain)?;
    capture_parts(&pkg, &parts_dir)?;

    if let Some(data) = pkg.get_part("[Content_Types].xml") {
        let overrides = Package::content_type_overrides(data).map_err(plain)?;
        if !overrides.is_empty() {
            let mut toml = String::from("[parts]\n");
            let mut uris: Vec<&String> = overrides.keys().collect();
            uris.sort();
            for uri in uris {
                let ct = overrides.get(uri).expect("key present");
                toml.push_str(&format!("{} = \"{}\"\n", toml_key(uri), ct));
            }
            std::fs::write(parts_dir.join("_content-types.toml"), toml).map_err(plain)?;
        }
    }

    let name = Path::new(template)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("deck");
    let config = format!("[presentation]\nname = \"{name}\"\n");
    std::fs::write(dir.join("config.toml"), config).map_err(plain)?;

    eprintln!("created project `{project}` from `{template}`");
    eprintln!(
        "  edit {}/PRESENTATION.md, then run `gwen build {}`",
        project, project
    );
    Ok(())
}

/// Escape a package URI for use as a bare TOML key.
fn toml_key(uri: &str) -> String {
    if uri
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        uri.to_string()
    } else {
        format!("\"{uri}\"")
    }
}
