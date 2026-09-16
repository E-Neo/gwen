//! Translate parsed content and config into the JSON spec consumed by the
//! embedded pptxgenjs bridge. All geometry is converted EMU -> inches.

use std::path::Path;

use serde_json::{Value, json};

use crate::config::{Align, Anchor, Config, Element, ShapeKind};
use crate::content::Slide;
use crate::error::Result;
use base64::Engine;

const EMU_PER_INCH: f64 = 914_400.0;

fn inches(emu: i64) -> f64 {
    emu as f64 / EMU_PER_INCH
}

/// Build the pptxgenjs spec for a whole deck.
pub fn spec(project: &Path, config: &Config, slides: &[Slide]) -> Result<Value> {
    let slides: Result<Vec<Value>> = slides
        .iter()
        .map(|slide| slide_value(project, config, slide))
        .collect();
    Ok(json!({
        "width": inches(config.presentation.slide_width),
        "height": inches(config.presentation.slide_height),
        "slides": slides?,
    }))
}

fn slide_value(project: &Path, config: &Config, slide: &Slide) -> Result<Value> {
    let layout = config.layout(&slide.layout)?;

    let mut shapes = Vec::new();
    for element in &layout.elements {
        match element {
            Element::Shape(s) => shapes.push(shape_value(s)),
            Element::Slot(slot) => match slot.slot.as_str() {
                "title" => {
                    if let Some(title) = &slide.title {
                        shapes.push(text_value(
                            slot,
                            &[para(title, false, 0)],
                            &config.theme.major_font,
                        ));
                    }
                }
                "subtitle" => {
                    if let Some(subtitle) = &slide.subtitle {
                        shapes.push(text_value(
                            slot,
                            &[para(subtitle, false, 0)],
                            &config.theme.minor_font,
                        ));
                    }
                }
                "body" if !slide.body.is_empty() => {
                    let paragraphs: Vec<Value> = slide
                        .body
                        .iter()
                        .map(|p| para(&p.text, p.bullet, p.level))
                        .collect();
                    shapes.push(text_value(slot, &paragraphs, &config.theme.minor_font));
                }
                "picture" => {
                    if let Some(filename) = slide.pictures.first() {
                        shapes.push(picture_value(project, slot, filename)?);
                    }
                }
                _ => {}
            },
        }
    }

    let mut out = json!({ "shapes": shapes });
    if let Some(background) = &slide.background {
        out["background"] = json!(background);
    }
    if let Some(notes) = &slide.notes {
        out["notes"] = json!(notes);
    }
    Ok(out)
}

fn para(text: &str, bullet: bool, level: i32) -> Value {
    json!({ "text": text, "bullet": bullet, "level": level })
}

fn shape_value(s: &crate::config::ShapeElement) -> Value {
    let line = match s.shape {
        ShapeKind::Line => Some(json!({
            "color": s.fill.as_deref().unwrap_or("000000"),
            "width": 1,
        })),
        _ => None,
    };
    json!({
        "kind": "shape",
        "preset": s.shape.preset(),
        "x": inches(s.left),
        "y": inches(s.top),
        "w": inches(s.width),
        "h": inches(s.height),
        "fill": if s.no_fill { Value::Null } else { json!(s.fill) },
        "line": line,
    })
}

fn text_value(slot: &crate::config::SlotElement, paragraphs: &[Value], font: &str) -> Value {
    json!({
        "kind": "text",
        "x": inches(slot.left),
        "y": inches(slot.top),
        "w": inches(slot.width),
        "h": inches(slot.height),
        "fontSize": slot.text_size,
        "color": slot.color,
        "bold": slot.bold,
        "italic": slot.italic,
        "fontFace": font,
        "align": align_str(slot.align),
        "anchor": anchor_str(slot.anchor),
        "wrap": slot.wrap.unwrap_or(true),
        "fill": slot.fill,
        "paragraphs": paragraphs,
    })
}

fn picture_value(
    project: &Path,
    slot: &crate::config::SlotElement,
    filename: &str,
) -> Result<Value> {
    let path = project.join("src").join("media").join(filename);
    let bytes = std::fs::read(&path)
        .map_err(|e| miette::miette!("cannot read `{}`: {e}", path.display()))?;
    let mime = mime_for(filename)?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(json!({
        "kind": "picture",
        "x": inches(slot.left),
        "y": inches(slot.top),
        "w": inches(slot.width),
        "h": inches(slot.height),
        "dataUri": format!("data:{mime};base64,{encoded}"),
    }))
}

fn mime_for(filename: &str) -> Result<&'static str> {
    let ext = filename.rsplit('.').next().unwrap_or("");
    match ext.to_ascii_lowercase().as_str() {
        "png" => Ok("image/png"),
        "jpg" | "jpeg" => Ok("image/jpeg"),
        "gif" => Ok("image/gif"),
        "bmp" => Ok("image/bmp"),
        "tif" | "tiff" => Ok("image/tiff"),
        "svg" => Ok("image/svg+xml"),
        other => Err(miette::miette!(
            "unsupported image extension `.{other}` for `{filename}`"
        )),
    }
}

fn align_str(a: Align) -> &'static str {
    match a {
        Align::Left => "left",
        Align::Center => "center",
        Align::Right => "right",
        Align::Justify => "justify",
    }
}

fn anchor_str(a: Anchor) -> &'static str {
    match a {
        Anchor::Top => "top",
        Anchor::Middle => "middle",
        Anchor::Bottom => "bottom",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Element, Layout, ShapeElement, SlotElement, SlotKind, parse_hex};
    use crate::content::Paragraph;
    use std::collections::BTreeMap;

    fn sample_config() -> Config {
        let mut layouts = BTreeMap::new();
        let elements = vec![
            Element::Shape(ShapeElement {
                shape: ShapeKind::Rect,
                left: 0,
                top: 0,
                width: 9144000,
                height: 914400,
                fill: Some(parse_hex("#C7000A").unwrap()),
                no_fill: false,
            }),
            Element::Slot(SlotElement {
                slot: "title".into(),
                kind: SlotKind::Text,
                left: 914400,
                top: 914400,
                width: 9144000,
                height: 914400,
                align: Align::Left,
                anchor: Anchor::Middle,
                text_size: Some(32),
                color: Some(parse_hex("#1D1D1A").unwrap()),
                bold: Some(true),
                italic: None,
                wrap: None,
                fill: None,
            }),
            Element::Slot(SlotElement {
                slot: "body".into(),
                kind: SlotKind::Text,
                left: 914400,
                top: 1828800,
                width: 9144000,
                height: 4114800,
                align: Align::Left,
                anchor: Anchor::Top,
                text_size: Some(20),
                color: Some(parse_hex("#262626").unwrap()),
                bold: None,
                italic: None,
                wrap: None,
                fill: None,
            }),
        ];
        layouts.insert("content".to_string(), Layout { elements });
        Config {
            presentation: crate::config::Presentation {
                name: "demo".into(),
                slide_width: 9_144_000,
                slide_height: 6_858_000,
                default_layout: "content".into(),
            },
            theme: crate::config::Theme::default(),
            layouts,
        }
    }

    #[test]
    fn spec_maps_geometry_and_text() {
        let config = sample_config();
        let slides = vec![Slide {
            source: "slides/s1.md".into(),
            layout: "content".into(),
            background: Some(parse_hex("#112233").unwrap()),
            title: Some("Hello".into()),
            subtitle: None,
            body: vec![Paragraph {
                text: "point".into(),
                level: 0,
                bullet: true,
            }],
            pictures: vec![],
            notes: Some("note".into()),
        }];
        let dir = std::env::temp_dir();
        let spec = spec(&dir, &config, &slides).unwrap();
        assert_eq!(spec["width"], 10.0);
        assert_eq!(spec["height"], 7.5);
        let s = &spec["slides"][0];
        assert_eq!(s["background"], "112233");
        assert_eq!(s["notes"], "note");
        let shapes = s["shapes"].as_array().unwrap();
        assert_eq!(shapes.len(), 3); // shape + title + body
        assert_eq!(shapes[0]["kind"], "shape");
        assert_eq!(shapes[0]["preset"], "rect");
        assert_eq!(shapes[0]["fill"], "C7000A");
        assert_eq!(shapes[1]["kind"], "text");
        assert_eq!(shapes[1]["paragraphs"][0]["text"], "Hello");
        assert_eq!(shapes[1]["fontFace"], "Calibri");
        assert_eq!(shapes[2]["paragraphs"][0]["bullet"], true);
        assert_eq!(shapes[2]["fontFace"], "Calibri");
    }
}
