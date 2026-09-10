//! `ppt/slides/slideN.xml`.

use std::collections::HashMap;

use crate::config::{Layout, SlotElement, SlotKind, Theme};
use crate::content::{Paragraph, Slide};
use crate::error::Result;
use crate::ooxml::Elem;
use crate::parts::{IdGen, picture_shape, solid_fill, sp_tree, text_shape};

/// Build a slide part. `image_rids` maps media filenames to their relationship
/// ids (assigned by the engine).
pub fn build(
    slide: &Slide,
    layout_key: &str,
    layout: &Layout,
    theme: &Theme,
    image_rids: &HashMap<String, String>,
) -> Result<Vec<u8>> {
    let mut ids = IdGen::new();
    let mut shapes = Vec::new();

    if let Some(title) = &slide.title {
        let slot = text_slot(slide, layout, layout_key, "title")?;
        let paragraph = Paragraph {
            text: title.clone(),
            level: 0,
            bullet: false,
        };
        shapes.push(text_shape(
            ids.take(),
            "Title",
            slot,
            &[paragraph],
            &theme.major_font,
        ));
    }
    if let Some(subtitle) = &slide.subtitle {
        let slot = text_slot(slide, layout, layout_key, "subtitle")?;
        let paragraph = Paragraph {
            text: subtitle.clone(),
            level: 0,
            bullet: false,
        };
        shapes.push(text_shape(
            ids.take(),
            "Subtitle",
            slot,
            &[paragraph],
            &theme.minor_font,
        ));
    }
    if !slide.body.is_empty() {
        let slot = text_slot(slide, layout, layout_key, "body")?;
        shapes.push(text_shape(
            ids.take(),
            "Body",
            slot,
            &slide.body,
            &theme.minor_font,
        ));
    }
    if let Some(filename) = slide.pictures.first() {
        let slot = picture_slot(slide, layout, layout_key)?;
        let rid = image_rids.get(filename).ok_or_else(|| {
            miette::miette!(
                "slide `{}`: picture `{filename}` was not found in `src/media`",
                slide.source
            )
        })?;
        shapes.push(picture_shape(ids.take(), "Picture", slot, rid));
    }

    let mut c_sld = Elem::new("p:cSld");
    if let Some(background) = &slide.background {
        c_sld = c_sld.child(
            Elem::new("p:bg").child(
                Elem::new("p:bgPr")
                    .child(solid_fill(background))
                    .child(Elem::new("a:effectLst")),
            ),
        );
    }
    c_sld = c_sld.child(sp_tree(shapes));

    Ok(Elem::new("p:sld")
        .attr(
            "xmlns:a",
            "http://schemas.openxmlformats.org/drawingml/2006/main",
        )
        .attr(
            "xmlns:r",
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
        )
        .attr(
            "xmlns:p",
            "http://schemas.openxmlformats.org/presentationml/2006/main",
        )
        .child(c_sld)
        .child(Elem::new("p:clrMapOvr").child(Elem::new("a:masterClrMapping")))
        .to_xml())
}

/// The package URI for slide number `n` (1-based).
pub fn uri(n: usize) -> String {
    format!("ppt/slides/slide{n}.xml")
}

fn find_slot<'a>(layout: &'a Layout, name: &str) -> Option<&'a SlotElement> {
    layout.elements.iter().find_map(|element| match element {
        crate::config::Element::Slot(slot) if slot.slot == name => Some(slot),
        _ => None,
    })
}

fn text_slot<'a>(
    slide: &Slide,
    layout: &'a Layout,
    layout_key: &str,
    name: &str,
) -> Result<&'a SlotElement> {
    let slot = find_slot(layout, name).ok_or_else(|| {
        miette::miette!(
            "slide `{}`: layout `{layout_key}` has no `{name}` slot for this content",
            slide.source
        )
    })?;
    if slot.kind != SlotKind::Text {
        return Err(miette::miette!(
            "slide `{}`: layout `{layout_key}` slot `{name}` is not a text slot",
            slide.source
        ));
    }
    Ok(slot)
}

fn picture_slot<'a>(
    slide: &Slide,
    layout: &'a Layout,
    layout_key: &str,
) -> Result<&'a SlotElement> {
    let slot = find_slot(layout, "picture").ok_or_else(|| {
        miette::miette!(
            "slide `{}`: layout `{layout_key}` has no `picture` slot",
            slide.source
        )
    })?;
    if slot.kind != SlotKind::Picture {
        return Err(miette::miette!(
            "slide `{}`: layout `{layout_key}` slot `picture` is not a picture slot",
            slide.source
        ));
    }
    Ok(slot)
}
