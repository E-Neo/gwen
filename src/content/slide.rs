//! Parse one slide Markdown file into the typed [`Slide`] model.
//!
//! Grammar:
//! - front matter sets `layout` (required) and `background` (`#RRGGBB`);
//! - `# Heading` → title, `## Subtitle` → subtitle;
//! - `## Notes` switches the rest of the file into the notes slide;
//! - paragraphs and `- ` bullets → body text;
//! - `![]()` → a picture (one per slide).

use std::collections::BTreeMap;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use super::{Paragraph, Slide};
use crate::config::parse_hex;
use crate::error::Result;

/// Parse a slide from its project-relative `source` path and file text.
pub fn parse(source: &str, text: &str) -> Result<Slide> {
    let (front, body) = split_front_matter(text);
    let layout = front
        .get("layout")
        .cloned()
        .ok_or_else(|| miette::miette!("slide `{source}`: front matter must set `layout`"))?;
    for key in front.keys() {
        if key != "layout" && key != "background" {
            return Err(miette::miette!(
                "slide `{source}`: unknown front matter key `{key}`"
            ));
        }
    }
    let background = match front.get("background") {
        Some(value) => Some(
            parse_hex(value).map_err(|e| miette::miette!("slide `{source}` background: {e}"))?,
        ),
        None => None,
    };

    let mut slide = Slide {
        source: source.to_string(),
        layout,
        background,
        title: None,
        subtitle: None,
        body: Vec::new(),
        pictures: Vec::new(),
        notes: None,
    };
    parse_body(source, body, &mut slide)?;
    if slide.pictures.len() > 1 {
        return Err(miette::miette!(
            "slide `{source}`: only one picture per slide is supported"
        ));
    }
    Ok(slide)
}

/// Split leading `---` front matter from the Markdown body.
fn split_front_matter(text: &str) -> (BTreeMap<String, String>, &str) {
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return (BTreeMap::new(), text);
    };
    let mut front = BTreeMap::new();
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if trimmed.trim() == "---" {
            return (front, &rest[offset + line.len()..]);
        }
        if let Some((key, value)) = trimmed.split_once(':') {
            front.insert(
                key.trim().to_string(),
                value.trim().trim_matches('"').to_string(),
            );
        }
        offset += line.len();
    }
    (BTreeMap::new(), text)
}

fn parse_body(source: &str, body: &str, slide: &mut Slide) -> Result<()> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    let parser = Parser::new_ext(body, options);

    let mut heading: Option<(u64, String)> = None;
    let mut paragraph: Option<String> = None;
    let mut list_depth: i32 = 0;
    let mut items: Vec<ItemRef> = Vec::new();
    let mut in_image = false;
    let mut in_notes = false;

    for event in parser {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                heading = Some((level as u64, String::new()));
            }
            Event::End(TagEnd::Heading(level)) => {
                let (lvl, raw) = heading.take().unwrap_or((level as u64, String::new()));
                let text = raw.trim().to_string();
                if in_notes {
                    notes_push(slide, &text);
                } else if lvl == 2 && text.eq_ignore_ascii_case("notes") {
                    in_notes = true;
                    slide.notes = Some(String::new());
                } else if lvl == 1 {
                    if slide.title.is_some() {
                        return Err(miette::miette!("slide `{source}`: more than one `#` title"));
                    }
                    slide.title = Some(text);
                } else if lvl == 2 {
                    if slide.subtitle.is_some() {
                        return Err(miette::miette!(
                            "slide `{source}`: more than one `##` subtitle"
                        ));
                    }
                    slide.subtitle = Some(text);
                }
            }
            Event::Start(Tag::Paragraph) => {
                if !in_image {
                    paragraph = Some(String::new());
                }
            }
            Event::End(TagEnd::Paragraph) => {
                let Some(raw) = paragraph.take() else {
                    continue;
                };
                let text = raw.trim().to_string();
                if text.is_empty() || !items.is_empty() {
                    continue;
                }
                if in_notes {
                    notes_push(slide, &text);
                } else {
                    slide.body.push(Paragraph {
                        text,
                        level: 0,
                        bullet: false,
                    });
                }
            }
            Event::Start(Tag::List(_)) => list_depth += 1,
            Event::End(TagEnd::List(_)) => list_depth -= 1,
            Event::Start(Tag::Item) => {
                let body_index = if in_notes {
                    None
                } else {
                    slide.body.push(Paragraph {
                        text: String::new(),
                        level: list_depth - 1,
                        bullet: true,
                    });
                    Some(slide.body.len() - 1)
                };
                items.push(ItemRef {
                    body_index,
                    text: String::new(),
                });
            }
            Event::End(TagEnd::Item) => {
                let Some(item) = items.pop() else {
                    continue;
                };
                match item.body_index {
                    Some(index) => {
                        let text = item.text.trim().to_string();
                        if text.is_empty() {
                            slide.body.remove(index);
                        } else {
                            slide.body[index].text = text;
                        }
                    }
                    None => notes_push(slide, &item.text),
                }
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                in_image = true;
                let filename = dest_url.rsplit('/').next().unwrap_or(&dest_url).to_string();
                if !filename.is_empty() {
                    slide.pictures.push(filename);
                }
            }
            Event::End(TagEnd::Image) => in_image = false,
            Event::Text(text) | Event::Code(text) => {
                if in_image {
                    continue;
                }
                if let Some((_, buffer)) = heading.as_mut() {
                    buffer.push_str(&text);
                } else if let Some(item) = items.last_mut() {
                    item.text.push_str(&text);
                } else if let Some(buffer) = paragraph.as_mut() {
                    buffer.push_str(&text);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// One open list item while parsing.
struct ItemRef {
    /// Index into `slide.body` for normal content; `None` while in notes.
    body_index: Option<usize>,
    text: String,
}

fn notes_push(slide: &mut Slide, text: &str) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    let notes = slide.notes.get_or_insert_with(String::new);
    if !notes.is_empty() {
        notes.push('\n');
    }
    notes.push_str(text);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_title_body_bullets_picture_and_notes() {
        let text = r##"---
layout: content
background: "#112233"
---

# Hello

Intro paragraph.

- one
- two
  - nested

![logo](media/logo.png)

## Notes

Speaker notes here.
"##;
        let slide = parse("slides/x.md", text).unwrap();
        assert_eq!(slide.layout, "content");
        assert_eq!(slide.background.as_deref(), Some("112233"));
        assert_eq!(slide.title.as_deref(), Some("Hello"));
        assert_eq!(slide.body.len(), 4);
        assert_eq!(slide.body[0].text, "Intro paragraph.");
        assert_eq!(slide.body[1].text, "one");
        assert!(slide.body[1].bullet);
        assert_eq!(slide.body[2].text, "two");
        assert_eq!(slide.body[3].text, "nested");
        assert_eq!(slide.body[3].level, 1);
        assert_eq!(slide.pictures, vec!["logo.png"]);
        assert_eq!(slide.notes.as_deref(), Some("Speaker notes here."));
    }

    #[test]
    fn requires_layout() {
        assert!(parse("slides/x.md", "# Hi\n").is_err());
    }
}
