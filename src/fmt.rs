//! `gwen fmt`: normalize the whitespace in a project's TOML files.
//!
//! The formatter works on `toml_edit`'s decor layer (the whitespace and
//! comments around keys, values and table headers), so it never re-serialises
//! a value: strings, numbers and multi-line strings come out byte-for-byte as
//! they went in. Key order, `[[shapes]]` / `[[sections]]` order and every
//! comment stay where the author put them.
//!
//! The style is the one the bundled template uses:
//!
//! - one space around `=`, after the comma in `[1, 2, 3]` and inside
//!   `{ color = "FFFFFF" }`;
//! - a multi-line array stays multi-line: one element per line, two spaces
//!   per nesting level, trailing comma;
//! - at most one consecutive blank line, and exactly one before each
//!   `[table]` / `[[table]]` header (before its comment block, if it has one);
//! - no leading blank line, no trailing whitespace, one final newline.

use std::path::{Path, PathBuf};

use gwen::error::Result;
use toml_edit::{Array, DocumentMut, InlineTable, Item, KeyMut, RawString, Table, Value};

/// Format every `*.toml` under `project`.
///
/// With `check`, nothing is written and the return value says whether any file
/// would change. Returns true when the caller should exit non-zero: a file
/// needs formatting in `--check` mode, or a file could not be parsed.
pub fn format(project: &str, check: bool) -> Result<bool> {
    let root = Path::new(project);
    if !root.join("main.toml").is_file() {
        return Err(miette::miette!(
            "`{}` is not a gwen project (no `main.toml`)",
            root.display()
        ));
    }

    let mut files = Vec::new();
    walk(root, &mut files)?;
    files.sort();

    let mut changed = 0usize;
    let mut unchanged = 0usize;
    let mut failed = 0usize;
    for path in &files {
        let rel = path.strip_prefix(root).unwrap_or(path);
        let src = match std::fs::read_to_string(path) {
            Ok(src) => src,
            Err(e) => {
                eprintln!("error: {}: {e}", rel.display());
                failed += 1;
                continue;
            }
        };
        let out = match format_source(&src) {
            Ok(out) => out,
            Err(e) => {
                eprintln!("error: {}: {e}", rel.display());
                failed += 1;
                continue;
            }
        };
        if out == src {
            unchanged += 1;
            continue;
        }
        changed += 1;
        if check {
            eprintln!("{:<13} {}", "needs format", rel.display());
        } else {
            if let Err(e) = std::fs::write(path, &out) {
                eprintln!("error: {}: {e}", rel.display());
                failed += 1;
                changed -= 1;
                continue;
            }
            eprintln!("{:<13} {}", "formatted", rel.display());
        }
    }

    let total = changed + unchanged + failed;
    if failed > 0 {
        eprintln!("error: {failed} of {total} files could not be formatted");
    }
    if changed == 0 {
        if failed == 0 {
            eprintln!("all {unchanged} files already formatted");
        }
    } else if check {
        eprintln!("{changed} of {total} files need formatting");
    } else {
        eprintln!("{changed} formatted, {unchanged} unchanged");
    }
    Ok(failed > 0 || (check && changed > 0))
}

/// All `*.toml` files below `dir`, skipping `target/` and `.git/`.
fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| miette::miette!("cannot list `{}`: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| miette::miette!("cannot read `{}`: {e}", dir.display()))?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let kind = entry
            .file_type()
            .map_err(|e| miette::miette!("cannot stat `{}`: {e}", path.display()))?;
        if kind.is_dir() {
            if name == "target" || name == ".git" {
                continue;
            }
            walk(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "toml") {
            out.push(path);
        }
    }
    Ok(())
}

/// Format one TOML document.
///
/// Multi-line arrays that contain comments are left exactly as they are: a
/// comment after an element belongs to that element's decor, and moving it
/// around would reorder the author's annotations.
fn format_source(src: &str) -> std::result::Result<String, toml_edit::TomlError> {
    let mut doc: DocumentMut = src.parse()?;
    format_table(doc.as_table_mut(), true);

    let trailing = raw_str(doc.trailing());
    let block = normalize_block(trailing);
    let mut tail = block.join("\n");
    if !tail.is_empty() {
        tail.push('\n');
    }
    doc.set_trailing(tail);

    let out = doc.to_string();
    let out = out.trim_start_matches('\n');
    let out = out.trim_end();
    if out.is_empty() {
        Ok(String::new())
    } else {
        Ok(format!("{out}\n"))
    }
}

/// How a value may be laid out.
#[derive(Clone, Copy, PartialEq)]
enum Layout {
    /// A value in a table body: arrays may use several lines.
    Block,
    /// A value inside an inline table: everything on one line.
    Inline,
}

fn raw_str(raw: &RawString) -> &str {
    raw.as_str().unwrap_or("")
}

fn decor_str(raw: Option<&RawString>) -> &str {
    raw.and_then(RawString::as_str).unwrap_or("")
}

/// Split a decor prefix into normalized lines: indentation and trailing
/// whitespace removed, runs of blank lines collapsed to one.
///
/// Everything after the last `\n` (indentation on the key's own line) is
/// dropped, because keys go at column 0.
fn normalize_block(raw: &str) -> Vec<String> {
    let Some(end) = raw.rfind('\n') else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for line in raw[..end].split('\n') {
        let line = line.trim();
        if line.is_empty() {
            if out.last().is_some_and(String::is_empty) {
                continue;
            }
            out.push(String::new());
        } else {
            out.push(line.to_string());
        }
    }
    out
}

/// The gap in front of a key: up to one blank line, then its comments.
fn key_prefix(raw: &str) -> String {
    let block = normalize_block(raw);
    if block.is_empty() {
        return String::new();
    }
    let mut out = block.join("\n");
    out.push('\n');
    out
}

/// The gap in front of a table header: exactly one blank line, then the
/// comment block that belongs to the header.
fn table_prefix(raw: &str) -> String {
    let mut block = normalize_block(raw);
    while block.first().is_some_and(String::is_empty) {
        block.remove(0);
    }
    let mut out = String::from("\n");
    for line in &block {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// The text after a value or header on the same line: its trailing comment,
/// preceded by exactly one space.
fn normalize_suffix(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!(" {trimmed}")
    }
}

fn has_comment(raw: Option<&RawString>) -> bool {
    decor_str(raw).contains('#')
}

/// Remove every bit of author spacing from a value, keeping its trailing
/// comment. The encoder then applies its canonical spacing.
fn normalize_value(value: &mut Value) {
    let suffix = normalize_suffix(decor_str(value.decor().suffix()));
    let decor = value.decor_mut();
    decor.clear();
    if !suffix.is_empty() {
        decor.set_suffix(suffix);
    }
}

fn array_is_multiline(array: &Array) -> bool {
    raw_str(array.trailing()).contains('\n')
        || array
            .iter()
            .any(|value| decor_str(value.decor().prefix()).contains('\n'))
}

fn array_has_comments(array: &Array) -> bool {
    has_comment(Some(array.trailing()))
        || array
            .iter()
            .any(|value| has_comment(value.decor().prefix()) || has_comment(value.decor().suffix()))
}

fn format_array(array: &mut Array, layout: Layout, level: usize) {
    if array.is_empty() {
        array.set_trailing("");
        array.set_trailing_comma(false);
        return;
    }

    let multiline = layout == Layout::Block && array_is_multiline(array);
    if multiline && array_has_comments(array) {
        return;
    }

    if multiline {
        let inner = "  ".repeat(level + 1);
        let close = "  ".repeat(level);
        for element in array.iter_mut() {
            let suffix = normalize_suffix(decor_str(element.decor().suffix()));
            format_value(element, Layout::Block, level + 1);
            let decor = element.decor_mut();
            decor.set_prefix(format!("\n{inner}"));
            decor.set_suffix(suffix);
        }
        array.set_trailing(format!("\n{close}"));
        array.set_trailing_comma(true);
    } else {
        for element in array.iter_mut() {
            format_value(element, Layout::Inline, level);
            element.decor_mut().clear();
        }
        array.set_trailing("");
        array.set_trailing_comma(false);
    }
}

/// Inline tables are always one line: TOML does not allow newlines in them.
fn format_inline_table(table: &mut InlineTable, level: usize) {
    for (mut key, value) in table.iter_mut() {
        key.leaf_decor_mut().clear();
        key.dotted_decor_mut().clear();
        format_value(value, Layout::Inline, level);
        normalize_value(value);
    }
    table.set_trailing("");
    table.set_trailing_comma(false);
}

fn format_value(value: &mut Value, layout: Layout, level: usize) {
    match value {
        Value::Array(array) => format_array(array, layout, level),
        Value::InlineTable(table) => format_inline_table(table, level),
        _ => {}
    }
}

/// Normalize the gap and comments above a key, and the space before `=`.
fn format_key(key: &mut KeyMut<'_>) {
    let prefix = key_prefix(decor_str(key.leaf_decor().prefix()));
    key.leaf_decor_mut().set_prefix(prefix);
    key.leaf_decor_mut().set_suffix(" ");
    key.dotted_decor_mut().clear();
}

fn format_table(table: &mut Table, root: bool) {
    if !root && !table.is_implicit() && !table.is_dotted() {
        let prefix = table_prefix(decor_str(table.decor().prefix()));
        let suffix = normalize_suffix(decor_str(table.decor().suffix()));
        let decor = table.decor_mut();
        decor.set_prefix(prefix);
        decor.set_suffix(suffix);
    }

    for (mut key, item) in table.iter_mut() {
        match item {
            Item::Value(value) => {
                format_key(&mut key);
                format_value(value, Layout::Block, 0);
                normalize_value(value);
            }
            // A dotted table has no header of its own; its key takes part in
            // the bodies above it, so it keeps a key's gap and comments.
            Item::Table(inner) if inner.is_dotted() => {
                format_key(&mut key);
                format_table(inner, false);
            }
            Item::Table(inner) => {
                key.leaf_decor_mut().clear();
                key.dotted_decor_mut().clear();
                format_table(inner, false);
            }
            Item::ArrayOfTables(array) => {
                key.leaf_decor_mut().clear();
                key.dotted_decor_mut().clear();
                for inner in array.iter_mut() {
                    format_table(inner, false);
                }
            }
            Item::None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(input: &str) -> String {
        format_source(input).expect("valid TOML")
    }

    /// Formatting must not change what the document means.
    fn assert_same_value(before: &str, after: &str) {
        assert_eq!(
            before.parse::<toml::Table>().unwrap(),
            after.parse::<toml::Table>().unwrap()
        );
    }

    fn assert_idempotent(input: &str) {
        let once = fmt(input);
        assert_eq!(fmt(&once), once, "second pass changed the output");
        assert_same_value(input, &once);
    }

    #[test]
    fn normalizes_spacing_and_trailing_whitespace() {
        let input = "a=1   \n b =  [ 1 ,2,  3 ]\nc = {x=1,y =  { z=2 } }\nhead = \"x\"\t\n";
        let out = fmt(input);
        assert_eq!(
            out,
            "a = 1\nb = [1, 2, 3]\nc = { x = 1, y = { z = 2 } }\nhead = \"x\"\n"
        );
        assert_idempotent(input);
    }

    #[test]
    fn headers_are_at_column_zero_with_one_blank_line() {
        let input = "\n\n  [ presentation ]\n title = \"x\"\n\n\n\n  [ styles . named . card ]\nfont_size=12\n";
        let out = fmt(input);
        assert_eq!(
            out,
            "[presentation]\ntitle = \"x\"\n\n[styles.named.card]\nfont_size = 12\n"
        );
        assert_idempotent(input);
    }

    #[test]
    fn header_comments_move_with_the_header() {
        let input = "a = 1\n\n\n# The card.\n# Two lines.\n[[shapes]]\ntype = \"rect\"\n# The next one.\n[[shapes]]\ntype = \"text\"\n";
        let out = fmt(input);
        assert_eq!(
            out,
            "a = 1\n\n# The card.\n# Two lines.\n[[shapes]]\ntype = \"rect\"\n\n# The next one.\n[[shapes]]\ntype = \"text\"\n"
        );
        assert_idempotent(input);
    }

    #[test]
    fn blank_lines_between_keys_collapse_to_one() {
        let input = "a = 1\n\n\n\nb = 2\n# note\n\n\n# more\nc = 3\n";
        let out = fmt(input);
        assert_eq!(out, "a = 1\n\nb = 2\n# note\n\n# more\nc = 3\n");
        assert_idempotent(input);
    }

    #[test]
    fn comments_keep_their_text_and_trailing_comment_gets_one_space() {
        let input = "a = 1    #   body   \n#   keep   inner   spacing\nb = 2\n";
        let out = fmt(input);
        assert_eq!(out, "a = 1 #   body\n#   keep   inner   spacing\nb = 2\n");
        assert_idempotent(input);
    }

    #[test]
    fn multi_line_strings_are_verbatim() {
        let input = "a = \"\"\"\n  key = 1   \n[not a table]\n# not a comment\n\n\n\"\"\"\nb=2\n";
        let out = fmt(input);
        assert_eq!(
            out,
            "a = \"\"\"\n  key = 1   \n[not a table]\n# not a comment\n\n\n\"\"\"\nb = 2\n"
        );
        assert_idempotent(input);
    }

    #[test]
    fn multi_line_arrays_get_one_element_per_line() {
        let input = "points = [\n  { x=\"0cm\",   y = \"1cm\" },\n    { x = \"2cm\", y = \"3cm\" }\n]\nnested = [[1,2],[ 3 , 4 ]]\n";
        let out = fmt(input);
        assert_eq!(
            out,
            "points = [\n  { x = \"0cm\", y = \"1cm\" },\n  { x = \"2cm\", y = \"3cm\" },\n]\nnested = [[1, 2], [3, 4]]\n"
        );
        assert_idempotent(input);
    }

    #[test]
    fn multi_line_arrays_keep_their_layout() {
        let input = "a = [1, 2]\nb = [\n  1,\n  2,\n]\n";
        let out = fmt(input);
        assert_eq!(out, "a = [1, 2]\nb = [\n  1,\n  2,\n]\n");
        assert_idempotent(input);
    }

    #[test]
    fn nested_multi_line_arrays_are_indented_by_depth() {
        let input = "a = [\n[\n1,\n2,\n],\n]\n";
        let out = fmt(input);
        assert_eq!(out, "a = [\n  [\n    1,\n    2,\n  ],\n]\n");
        assert_idempotent(input);
    }

    #[test]
    fn arrays_with_comments_are_left_alone() {
        let input = "a = [\n  1,  # one\n  2,\n]\nb=1\n";
        let out = fmt(input);
        assert_eq!(out, "a = [\n  1,  # one\n  2,\n]\nb = 1\n");
        assert_idempotent(input);
    }

    #[test]
    fn empty_arrays_stay_on_one_line() {
        let input = "a = [\n\n]\nb = [ ]\n";
        assert_eq!(fmt(input), "a = []\nb = []\n");
        assert_idempotent(input);
    }

    #[test]
    fn trailing_comments_after_arrays_survive() {
        let input = "a = [ 1, 2 ]    #  after\n";
        assert_eq!(fmt(input), "a = [1, 2] #  after\n");
        assert_idempotent(input);
    }

    #[test]
    fn file_ends_with_exactly_one_newline() {
        assert_eq!(fmt("a = 1"), "a = 1\n");
        assert_eq!(fmt("a = 1\n\n\n"), "a = 1\n");
        assert_eq!(fmt("\n\na = 1\n"), "a = 1\n");
    }

    #[test]
    fn empty_document_stays_empty() {
        assert_eq!(fmt(""), "");
        assert_eq!(fmt("\n\n"), "");
    }

    #[test]
    fn sections_and_shapes_are_not_reordered() {
        let input = "[[sections]]\ntitle = \"B\"\nslides = []\n\n[[sections]]\ntitle = \"A\"\nslides = []\n\n[[shapes]]\ntype = \"rect\"\n\n[[shapes]]\ntype = \"text\"\n";
        assert_eq!(fmt(input), input);
        assert_idempotent(input);
    }
}
