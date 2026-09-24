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

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use crate::opts::MARKER_PATTERNS;

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
pub fn parse_paragraphs(
    text: &str,
    ordered_markers: &[String],
    unordered_markers: &[String],
) -> Vec<Vec<Run>> {
    let parser = Parser::new_ext(text, Options::ENABLE_STRIKETHROUGH);
    let mut paragraphs: Vec<Vec<Run>> = Vec::new();
    let mut current: Vec<Run> = Vec::new();
    let mut state = RunState::default();

    let push_text = |runs: &mut Vec<Run>,
                     hyperlink: &mut Option<String>,
                     bold: bool,
                     italic: bool,
                     s: &str,
                     soft_before: bool| {
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
            Event::Text(t) => {
                push_text(
                    &mut current,
                    &mut state.hyperlink,
                    state.bold,
                    state.italic,
                    &t,
                    pending_break,
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
                );
                pending_break = false;
            }
            Event::SoftBreak | Event::HardBreak => {
                pending_break = true;
            }
            _ => {}
        }
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
        }]);
    }
    paragraphs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(s: &str) -> Run {
        Run {
            text: s.into(),
            bold: false,
            italic: false,
            soft_break_before: None,
            hyperlink: None,
            bullet: None,
            indent_level: None,
        }
    }

    #[test]
    fn plain_text_single_paragraph() {
        let p = parse_paragraphs("Hello, world", &[], &[]);
        assert_eq!(p, vec![vec![plain("Hello, world")]]);
    }

    #[test]
    fn newline_is_soft_break() {
        let p = parse_paragraphs("a\nb", &[], &[]);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0][0].text, "a");
        assert_eq!(p[0][1].soft_break_before, Some(true));
        assert_eq!(p[0][1].text, "b");
    }

    #[test]
    fn blank_line_splits_paragraphs() {
        let p = parse_paragraphs("para one\n\npara two", &[], &[]);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0][0].text, "para one");
        assert_eq!(p[1][0].text, "para two");
    }

    #[test]
    fn emphasis_and_strong() {
        let p = parse_paragraphs("*it* **bold**", &[], &[]);
        assert!(p[0][0].italic);
        assert_eq!(p[0][1].text, " ");
        assert!(p[0][2].bold);
    }

    #[test]
    fn hyperlink_run() {
        let p = parse_paragraphs("[site](https://example.com/)", &[], &[]);
        let run = &p[0][0];
        assert_eq!(run.text, "site");
        assert_eq!(
            run.hyperlink,
            Some(serde_json::json!({ "url": "https://example.com/" }))
        );
    }

    #[test]
    fn tight_list_renders_items_with_bullets() {
        let p = parse_paragraphs("abc\n- a\n- b\n- c\n  1. x\n  2. y\n  3. z", &[], &[]);
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
        let p = parse_paragraphs("- a\n- b\n  1. x\n  2. y", &["A.".into()], &[]);
        let bullet = p[2][0].bullet.as_ref().unwrap();
        assert_eq!(bullet["style"], "alphaUcPeriod");
        assert_eq!(p[2][0].indent_level, Some(1));
        assert!(p[0][0].bullet == Some(serde_json::json!(true)));
    }

    #[test]
    fn unordered_marker_rune_becomes_character_code() {
        let p = parse_paragraphs("- a\n- b", &[], &["►".into()]);
        assert_eq!(
            p[0][0].bullet,
            Some(serde_json::json!({ "characterCode": "25BA" }))
        );
    }
}
