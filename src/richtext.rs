//! Markdown-flavoured rich text (reuses the battle-tested `pulldown-cmark`
//! parser). `text` values are parsed as inline markdown:
//!
//! - `*italic*`, `**bold**`, `[link](url)`, `` `code` `` -> text runs;
//! - a single `\n` -> a soft line break (pptxgenjs `softBreakBefore` run);
//! - a blank line (`\n\n`) -> a new paragraph.
//!
//! PPTX output is only ever produced by the pptxgenjs API; this module just
//! decides which run options to pass to it, never any OOXML.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

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

/// Parse markdown-flavoured `text` into a list of paragraphs, each a list of runs.
///
/// The paragraph boundary (`\n\n`), which `pulldown-cmark` treats as a document
/// structure event, is turned into a paragraph for `addText`. Everything
/// inline (emphasis, `\n` soft breaks, links, code) stays within the paragraph.
pub fn parse_paragraphs(text: &str) -> Vec<Vec<Run>> {
    let opts = Options::ENABLE_STRIKETHROUGH;
    let parser = Parser::new_ext(text, opts);
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
        });
    };

    // We walk the tree and buffer a potential soft break between runs. A break
    // is only emitted once a following text run actually appears; trailing
    // breaks before a paragraph end are dropped.
    let mut pending_break = false;

    for event in parser {
        match event {
            Event::Start(tag) => {
                state.push(&tag);
            }
            Event::End(tag_end) => {
                state.pop(&tag_end);
                if matches!(tag_end, TagEnd::Paragraph) {
                    if !current.is_empty() {
                        paragraphs.push(std::mem::take(&mut current));
                    }
                    pending_break = false;
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
        }
    }

    #[test]
    fn plain_text_single_paragraph() {
        let p = parse_paragraphs("Hello, world");
        assert_eq!(p, vec![vec![plain("Hello, world")]]);
    }

    #[test]
    fn newline_is_soft_break() {
        let p = parse_paragraphs("a\nb");
        assert_eq!(p.len(), 1);
        assert_eq!(p[0][0].text, "a");
        assert_eq!(p[0][1].soft_break_before, Some(true));
        assert_eq!(p[0][1].text, "b");
    }

    #[test]
    fn blank_line_splits_paragraphs() {
        let p = parse_paragraphs("para one\n\npara two");
        assert_eq!(p.len(), 2);
        assert_eq!(p[0][0].text, "para one");
        assert_eq!(p[1][0].text, "para two");
    }

    #[test]
    fn emphasis_and_strong() {
        let p = parse_paragraphs("*it* **bold**");
        assert!(p[0][0].italic);
        assert_eq!(p[0][1].text, " ");
        assert!(p[0][2].bold);
    }

    #[test]
    fn hyperlink_run() {
        let p = parse_paragraphs("[site](https://example.com/)");
        let run = &p[0][0];
        assert_eq!(run.text, "site");
        assert_eq!(
            run.hyperlink,
            Some(serde_json::json!({ "url": "https://example.com/" }))
        );
    }
}
