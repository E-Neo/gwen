//! Parse `SUMMARY.md` (mdbook-style) into an ordered list of slide paths.
//! Only top-level `- [label](path)` bullets count; headings, prose and nested
//! bullets are ignored.

/// Extract the slide paths in document order.
pub fn parse(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        // Top-level bullets only: no leading whitespace.
        if line.starts_with(char::is_whitespace) {
            continue;
        }
        let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) else {
            continue;
        };
        if let Some(path) = link_path(rest.trim()) {
            out.push(path);
        }
    }
    out
}

/// Extract the target of the first Markdown link in `text`.
fn link_path(text: &str) -> Option<String> {
    let open = text.find("](")?;
    let rest = &text[open + 2..];
    let close = rest.find(')')?;
    let path = rest[..close].trim();
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_top_level_bullets_in_order() {
        let md = "# Summary\n\n- [Title](slides/title.md)\n- [Intro](slides/intro.md)\n  - [Nested](slides/nested.md)\n\nprose\n";
        assert_eq!(parse(md), vec!["slides/title.md", "slides/intro.md"]);
    }
}
