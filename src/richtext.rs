//! Markdown-flavoured rich text (reuses the battle-tested `pulldown-cmark`
//! parser). `text` values are parsed as inline markdown:
//!
//! - `*italic*`, `**bold**`, `[link](url)`, `` `code` `` -> text runs;
//! - a single `\n` -> a soft line break (pptxgenjs `softBreakBefore` run);
//! - a blank line (`\n\n`) -> a new paragraph;
//! - `- item` / `1. item` markdown lists -> bulleted paragraphs whose marker
//!   style and indent level come from the configured list markers.
//!
//! PPTX output is only ever produced by the pptxgenjs API; this module just
//! decides which run options to pass to it, never any OOXML.

use std::collections::BTreeMap;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use crate::opts::MARKER_PATTERNS;

/// The pptxgenjs `TextRunOptions` a `<span class="...">` named style may set.
const SPAN_OPTION_KEYS: &[&str] = &[
    "bold",
    "italic",
    "strike",
    "superscript",
    "subscript",
    "color",
    "highlight",
    "font_face",
    "font_size",
    "underline",
    "char_spacing",
    "baseline",
    "lang",
    "rtl_mode",
];

/// One text run, mirroring pptxgenjs `TextRun`/`TextRunOptions`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub text: String,
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub bold: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub italic: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub soft_break_before: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub hyperlink: Option<serde_json::Value>,
    /// pptxgenjs `bullet` option for the paragraph's first run (`true`, a
    /// `{ characterCode }`, or an auto-number object).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub bullet: Option<serde_json::Value>,
    /// pptxgenjs `indentLevel` for list items (0 = top level).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub indent_level: Option<i64>,
    /// Extra per-run `TextRunOptions` (camelCase), e.g. a `<span>`'s named
    /// style's `color`/`font_face`/... Applied to every run inside the span.
    #[serde(flatten, skip_serializing_if = "BTreeMap::is_empty", default)]
    pub options: BTreeMap<String, serde_json::Value>,
}

/// Convert a named-style value (TOML) into the JSON run option pptxgenjs sees.
fn toml_to_json(v: &toml::Value) -> serde_json::Value {
    match v {
        toml::Value::String(s) => serde_json::Value::String(s.clone()),
        toml::Value::Integer(i) => serde_json::json!(*i),
        toml::Value::Float(f) => serde_json::json!(*f),
        toml::Value::Boolean(b) => serde_json::Value::Bool(*b),
        toml::Value::Array(a) => serde_json::Value::Array(a.iter().map(toml_to_json).collect()),
        toml::Value::Table(t) => serde_json::Value::Object(
            t.iter()
                .map(|(k, v)| (crate::opts::snake_to_camel(k), toml_to_json(v)))
                .collect(),
        ),
        toml::Value::Datetime(d) => serde_json::Value::String(d.to_string()),
    }
}

/// A span tag seen in the markdown: `<span class="...">` or `</span>`.
enum SpanOp {
    Open(String),
    Close,
}

/// Classify an inline-HTML event: a gwen text `<span>` (with its classes) or
/// a closing tag; anything else is left alone.
fn parse_span_tag(h: &str) -> Option<SpanOp> {
    let t = h.trim();
    let lower = t.to_ascii_lowercase();
    if lower.starts_with("</span") {
        return Some(SpanOp::Close);
    }
    if lower.starts_with("<span") {
        let classes = extract_class(t)?;
        return Some(SpanOp::Open(classes.to_string()));
    }
    None
}

/// Extract the `class="..."` attribute value of an HTML tag.
fn extract_class(tag: &str) -> Option<&str> {
    let t = tag.trim_end().strip_suffix('>')?.trim_end();
    let attrs = t
        .split_once(|c: char| c.is_whitespace())
        .map(|(_, a)| a)
        .unwrap_or("");
    let mut rest = attrs.trim_start();
    loop {
        rest = rest.trim_start();
        let eq = rest.find('=')?;
        let name = rest[..eq].trim();
        let after = rest[eq + 1..].trim_start();
        let quote = after.chars().next()?;
        if quote != '"' && quote != '\'' {
            rest = after;
            continue;
        }
        let end = after[1..].find(quote)?;
        let value = &after[1..=end];
        if name.eq_ignore_ascii_case("class") {
            return Some(value);
        }
        rest = &after[end + 2..];
    }
}

/// Resolve a span's space-separated class names into merged run options
/// (`class="a b"` applies `a` then `b`, later wins).
fn span_options(
    classes: &str,
    styles: &BTreeMap<String, toml::Table>,
) -> Result<BTreeMap<String, serde_json::Value>, String> {
    let mut merged = BTreeMap::new();
    for name in classes.split_whitespace() {
        let table = styles.get(name).ok_or_else(|| {
            format!("unknown span style `{name}` - define [styles.named.{name}] in main.toml")
        })?;
        for (k, v) in table {
            if !SPAN_OPTION_KEYS.contains(&k.as_str()) {
                return Err(format!(
                    "style `{name}` used in a <span> has key `{k}`, which is not a text-run option. \
                     Spans may only use run options: {}. Move `{k}` to the shape, or pick a different style for the span.",
                    SPAN_OPTION_KEYS.join(", ")
                ));
            }
            merged.insert(crate::opts::snake_to_camel(k), toml_to_json(v));
        }
    }
    Ok(merged)
}

/// A stack of inline-formatting states while walking `pulldown-cmark` events.
#[derive(Debug, Default, Clone)]
struct RunState {
    bold: bool,
    italic: bool,
    hyperlink: Option<String>,
}

impl RunState {
    fn push(&mut self, tag: &Tag) {
        match tag {
            Tag::Strong => self.bold = true,
            Tag::Emphasis => self.italic = true,
            Tag::Link { dest_url, .. } => self.hyperlink = Some(dest_url.to_string()),
            _ => {}
        }
    }

    fn pop(&mut self, tag: &TagEnd) {
        match tag {
            TagEnd::Strong => self.bold = false,
            TagEnd::Emphasis => self.italic = false,
            TagEnd::Link => self.hyperlink = None,
            _ => {}
        }
    }
}

/// The marker for the list an item belongs to (`Tag::List(Some(start))` is
/// ordered starting at `start`; `None` is an unordered bullet).
fn item_bullet(
    list: Option<Option<u64>>,
    ordered_markers: &[String],
    unordered_markers: &[String],
    indent: usize,
) -> Option<serde_json::Value> {
    match list {
        None => None,
        Some(None) => match unordered_markers.get(indent) {
            Some(ch) => {
                let hex = format!("{:04X}", ch.chars().next().unwrap() as u32);
                Some(serde_json::json!({ "characterCode": hex }))
            }
            None => Some(serde_json::json!(true)),
        },
        Some(Some(start)) => {
            let style = ordered_markers
                .get(indent)
                .map(|p| number_type(p))
                .or_else(|| ordered_markers.last().map(|p| number_type(p)))
                .unwrap_or("arabicPeriod");
            Some(serde_json::json!({
                "type": "number",
                "style": style,
                "numberStartAt": start,
            }))
        }
    }
}

fn number_type(pattern: &str) -> &'static str {
    MARKER_PATTERNS
        .iter()
        .find(|(p, _)| *p == pattern)
        .map(|(_, t)| *t)
        .unwrap_or("arabicPeriod")
}

/// Push `current` as a paragraph, attaching the pending list-marker bullet
/// and indent level (a loose list item's paragraphs all share them).
fn flush(
    paragraphs: &mut Vec<Vec<Run>>,
    current: &mut Vec<Run>,
    pending: &mut Option<(Option<serde_json::Value>, i64)>,
) {
    if current.is_empty() {
        return;
    }
    let mut runs = std::mem::take(current);
    if let Some((bullet, indent)) = pending
        && let Some(first) = runs.first_mut()
    {
        first.bullet = bullet.clone();
        first.indent_level = Some(*indent);
    }
    paragraphs.push(runs);
}

/// Parse markdown-flavoured `text` into a list of paragraphs, each a list of
/// runs. `ordered_markers`/`unordered_markers` are the per-indent-level
/// display patterns for ordered lists and bullet characters for unordered
/// lists; an empty slice means defaults (`"1."` / standard bullet).
///
/// `<span class="...">...</span>` applies the `[styles.named.<...>]` run
/// options to the runs inside (span is transparent to markdown, so both
/// `<span class="red">**bold**</span>` and `**<span class="red">bold</span>**`
/// work; `\<span` escapes to a literal angle bracket). Returns an error for an
/// unknown class, a style that is not run-convertible, or unbalanced tags.
pub fn parse_paragraphs(
    text: &str,
    ordered_markers: &[String],
    unordered_markers: &[String],
    styles: &BTreeMap<String, toml::Table>,
) -> Result<Vec<Vec<Run>>, String> {
    let parser = Parser::new_ext(text, Options::ENABLE_STRIKETHROUGH);
    let mut paragraphs: Vec<Vec<Run>> = Vec::new();
    let mut current: Vec<Run> = Vec::new();
    let mut state = RunState::default();
    // Open `<span>` styles, innermost last.
    let mut span_stack: Vec<BTreeMap<String, serde_json::Value>> = Vec::new();

    let push_text = |runs: &mut Vec<Run>,
                     hyperlink: &mut Option<String>,
                     bold: bool,
                     italic: bool,
                     s: &str,
                     soft_before: bool,
                     options: &BTreeMap<String, serde_json::Value>| {
        let text = s.to_string();
        if text.is_empty() {
            return;
        }
        runs.push(Run {
            text,
            bold,
            italic,
            soft_break_before: soft_before.then_some(true),
            hyperlink: hyperlink
                .take()
                .map(|url| serde_json::json!({ "url": url })),
            bullet: None,
            indent_level: None,
            options: options.clone(),
        });
    };

    // Buffered soft break between runs (dropped at a paragraph end).
    let mut pending_break = false;
    // Open list types (`None` = bullet, `Some(start)` = ordered).
    let mut list_stack: Vec<Option<u64>> = Vec::new();
    // Number of currently open list items: an item's indent level.
    let mut open_items = 0usize;
    // The bullet/indent of the item currently being walked.
    let mut pending: Option<(Option<serde_json::Value>, i64)> = None;

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::List(start) => list_stack.push(start),
                Tag::Item => {
                    // A new (possibly nested) item closes any content left
                    // over from the previous tight item.
                    flush(&mut paragraphs, &mut current, &mut pending);
                    let indent = open_items as i64;
                    open_items += 1;
                    pending = Some((
                        item_bullet(
                            list_stack.last().copied(),
                            ordered_markers,
                            unordered_markers,
                            indent as usize,
                        ),
                        indent,
                    ));
                }
                other => state.push(&other),
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::List(_) => {
                    list_stack.pop();
                }
                TagEnd::Item => {
                    flush(&mut paragraphs, &mut current, &mut pending);
                    open_items -= 1;
                    pending = None;
                }
                TagEnd::Paragraph => {
                    flush(&mut paragraphs, &mut current, &mut pending);
                    pending_break = false;
                }
                other => state.pop(&other),
            },
            Event::Html(h) | Event::InlineHtml(h) => {
                let tag = &*h;
                match parse_span_tag(tag) {
                    Some(SpanOp::Open(classes)) => {
                        let options = span_options(&classes, styles)?;
                        let mut merged = span_stack.last().cloned().unwrap_or_default();
                        merged.extend(options);
                        span_stack.push(merged);
                    }
                    Some(SpanOp::Close) if span_stack.pop().is_none() => {
                        return Err("stray </span> without a matching <span class=\"...\">".into());
                    }
                    Some(SpanOp::Close) => {}
                    None => {}
                }
            }
            Event::Text(t) => {
                push_text(
                    &mut current,
                    &mut state.hyperlink,
                    state.bold,
                    state.italic,
                    &t,
                    pending_break,
                    span_stack.last().unwrap_or(&BTreeMap::new()),
                );
                pending_break = false;
            }
            Event::Code(t) => {
                push_text(
                    &mut current,
                    &mut state.hyperlink,
                    state.bold,
                    state.italic,
                    &t,
                    pending_break,
                    span_stack.last().unwrap_or(&BTreeMap::new()),
                );
                pending_break = false;
            }
            Event::SoftBreak | Event::HardBreak => {
                pending_break = true;
            }
            _ => {}
        }
    }
    if !span_stack.is_empty() {
        return Err("unclosed <span class=\"...\"> - add a matching </span>".into());
    }

    // A final empty paragraph if the input ends with a blank line.
    if current.is_empty()
        && (text.is_empty()
            || text.ends_with("\n\n")
            || text.ends_with('\n') && paragraphs.is_empty())
    {
        paragraphs.push(vec![Run {
            text: String::new(),
            bold: false,
            italic: false,
            soft_break_before: None,
            hyperlink: None,
            bullet: None,
            indent_level: None,
            options: BTreeMap::new(),
        }]);
    }
    Ok(paragraphs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_styles() -> BTreeMap<String, toml::Table> {
        BTreeMap::new()
    }

    fn plain(s: &str) -> Run {
        Run {
            text: s.into(),
            bold: false,
            italic: false,
            soft_break_before: None,
            hyperlink: None,
            bullet: None,
            indent_level: None,
            options: BTreeMap::new(),
        }
    }

    fn parse(s: &str) -> Vec<Vec<Run>> {
        parse_paragraphs(s, &[], &[], &no_styles()).unwrap()
    }

    #[test]
    fn plain_text_single_paragraph() {
        let p = parse("Hello, world");
        assert_eq!(p, vec![vec![plain("Hello, world")]]);
    }

    #[test]
    fn newline_is_soft_break() {
        let p = parse("a\nb");
        assert_eq!(p.len(), 1);
        assert_eq!(p[0][0].text, "a");
        assert_eq!(p[0][1].soft_break_before, Some(true));
        assert_eq!(p[0][1].text, "b");
    }

    #[test]
    fn blank_line_splits_paragraphs() {
        let p = parse("para one\n\npara two");
        assert_eq!(p.len(), 2);
        assert_eq!(p[0][0].text, "para one");
        assert_eq!(p[1][0].text, "para two");
    }

    #[test]
    fn emphasis_and_strong() {
        let p = parse("*it* **bold**");
        assert!(p[0][0].italic);
        assert_eq!(p[0][1].text, " ");
        assert!(p[0][2].bold);
    }

    #[test]
    fn hyperlink_run() {
        let p = parse("[site](https://example.com/)");
        let run = &p[0][0];
        assert_eq!(run.text, "site");
        assert_eq!(
            run.hyperlink,
            Some(serde_json::json!({ "url": "https://example.com/" }))
        );
    }

    #[test]
    fn tight_list_renders_items_with_bullets() {
        let p = parse("abc\n- a\n- b\n- c\n  1. x\n  2. y\n  3. z");
        assert_eq!(
            p.iter().map(|par| par[0].text.as_str()).collect::<Vec<_>>(),
            vec!["abc", "a", "b", "c", "x", "y", "z"]
        );
        // top-level bullet items: standard bullet, indent 0
        assert!(p[1][0].bullet == Some(serde_json::json!(true)));
        assert_eq!(p[1][0].indent_level, Some(0));
        // nested ordered items: auto-number, indent 1 (parent 0 + 1)
        let nested = &p[4][0];
        assert!(nested.bullet.as_ref().unwrap()["type"] == "number");
        assert_eq!(nested.bullet.as_ref().unwrap()["style"], "arabicPeriod");
        assert_eq!(nested.indent_level, Some(1));
        // plain paragraph has no bullet
        assert!(p[0][0].bullet.is_none());
    }

    #[test]
    fn ordered_marker_override_applies_by_level() {
        let p = parse_paragraphs(
            "- a\n- b\n  1. x\n  2. y",
            &["A.".into()],
            &[],
            &no_styles(),
        )
        .unwrap();
        let bullet = p[2][0].bullet.as_ref().unwrap();
        assert_eq!(bullet["style"], "alphaUcPeriod");
        assert_eq!(p[2][0].indent_level, Some(1));
        assert!(p[0][0].bullet == Some(serde_json::json!(true)));
    }

    #[test]
    fn unordered_marker_rune_becomes_character_code() {
        let p = parse_paragraphs("- a\n- b", &[], &["►".into()], &no_styles()).unwrap();
        assert_eq!(
            p[0][0].bullet,
            Some(serde_json::json!({ "characterCode": "25BA" }))
        );
    }

    #[test]
    fn span_styles_apply_to_enclosed_runs() {
        let mut styles = BTreeMap::new();
        styles.insert(
            "red".into(),
            toml::Table::from_iter([
                ("color".into(), toml::Value::String("FF0000".into())),
                ("font_size".into(), toml::Value::Integer(20)),
            ]),
        );
        // markdown inside the span
        let p = parse_paragraphs(
            "black <span class=\"red\">**bold and red**</span>",
            &[],
            &[],
            &styles,
        )
        .unwrap();
        let runs = &p[0];
        assert_eq!(runs[0].text, "black ");
        assert!(runs[0].options.is_empty());
        assert_eq!(runs[1].text, "bold and red");
        assert!(runs[1].bold);
        assert_eq!(runs[1].options["color"], "FF0000");
        assert_eq!(runs[1].options["fontSize"], 20);

        // span inside the markdown markers
        let p =
            parse_paragraphs("**<span class=\"red\">red bold</span>**", &[], &[], &styles).unwrap();
        assert!(p[0][0].bold);
        assert_eq!(p[0][0].options["color"], "FF0000");
    }

    #[test]
    fn span_multiple_classes_merge_later_wins() {
        let mut styles = BTreeMap::new();
        styles.insert(
            "a".into(),
            toml::Table::from_iter([("color".into(), toml::Value::String("111111".into()))]),
        );
        styles.insert(
            "b".into(),
            toml::Table::from_iter([("color".into(), toml::Value::String("222222".into()))]),
        );
        let p = parse_paragraphs("<span class=\"a b\">x</span>", &[], &[], &styles).unwrap();
        assert_eq!(p[0][0].options["color"], "222222");
    }

    #[test]
    fn span_unknown_class_errors() {
        let err =
            parse_paragraphs("<span class=\"nope\">x</span>", &[], &[], &no_styles()).unwrap_err();
        assert!(err.contains("unknown span style `nope`"), "{err}");
        assert!(err.contains("styles.named.nope"), "{err}");
    }

    #[test]
    fn span_non_run_option_errors() {
        let mut styles = BTreeMap::new();
        styles.insert(
            "box".into(),
            toml::Table::from_iter([("fill".into(), toml::Value::String("FF0000".into()))]),
        );
        let err = parse_paragraphs("<span class=\"box\">x</span>", &[], &[], &styles).unwrap_err();
        assert!(
            err.contains("`fill`, which is not a text-run option"),
            "{err}"
        );
        assert!(err.contains("text-run option"), "{err}");
    }

    #[test]
    fn span_unbalanced_errors() {
        let mut styles = BTreeMap::new();
        styles.insert(
            "red".into(),
            toml::Table::from_iter([("color".into(), toml::Value::String("FF0000".into()))]),
        );
        let err = parse_paragraphs("<span class=\"red\">x", &[], &[], &styles).unwrap_err();
        assert!(err.contains("unclosed <span"), "{err}");
        let err = parse_paragraphs("x</span>", &[], &[], &styles).unwrap_err();
        assert!(err.contains("stray </span>"), "{err}");
    }

    #[test]
    fn escaped_span_is_literal() {
        let p = parse("\\<span class=\"red\">literal\\</span>");
        let text = p[0]
            .iter()
            .map(|r| r.text.as_str())
            .collect::<Vec<_>>()
            .join("");
        assert_eq!(text, "<span class=\"red\">literal</span>");
        assert!(p[0].iter().all(|r| r.options.is_empty()));
    }
}
