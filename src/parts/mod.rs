//! Part builders: one module per generated OOXML part, plus the shared shape
//! helpers every part uses.

pub mod core;
pub mod layout;
pub mod master;
pub mod notes;
pub mod presentation;
pub mod props;
pub mod slide;
pub mod theme;

use crate::config::{ShapeElement, ShapeKind, SlotElement};
use crate::content::Paragraph;
use crate::ooxml::Elem;

/// A per-part unique shape id counter (id 1 is the group shape).
pub struct IdGen(u32);

impl IdGen {
    pub fn new() -> Self {
        IdGen(2)
    }
    pub fn take(&mut self) -> u32 {
        let id = self.0;
        self.0 += 1;
        id
    }
}

impl Default for IdGen {
    fn default() -> Self {
        IdGen::new()
    }
}

/// The `p:spTree` group preamble plus `children`.
pub fn sp_tree(children: Vec<Elem>) -> Elem {
    Elem::new("p:spTree")
        .child(
            Elem::new("p:nvGrpSpPr")
                .child(Elem::new("p:cNvPr").attr("id", "1").attr("name", ""))
                .child(Elem::new("p:cNvGrpSpPr"))
                .child(Elem::new("p:nvPr")),
        )
        .child(
            Elem::new("p:grpSpPr").child(
                Elem::new("a:xfrm")
                    .child(Elem::new("a:off").attr("x", "0").attr("y", "0"))
                    .child(Elem::new("a:ext").attr("cx", "0").attr("cy", "0"))
                    .child(Elem::new("a:chOff").attr("x", "0").attr("y", "0"))
                    .child(Elem::new("a:chExt").attr("cx", "0").attr("cy", "0")),
            ),
        )
        .children(children)
}

pub fn xfrm(left: i64, top: i64, width: i64, height: i64) -> Elem {
    Elem::new("a:xfrm")
        .child(
            Elem::new("a:off")
                .attr("x", left.to_string())
                .attr("y", top.to_string()),
        )
        .child(
            Elem::new("a:ext")
                .attr("cx", width.to_string())
                .attr("cy", height.to_string()),
        )
}

pub fn solid_fill(hex: &str) -> Elem {
    Elem::new("a:solidFill").child(Elem::new("a:srgbClr").attr("val", hex))
}

pub fn prst_geom(preset: &str) -> Elem {
    Elem::new("a:prstGeom")
        .attr("prst", preset)
        .child(Elem::new("a:avLst"))
}

/// A decorative shape drawn on a layout.
pub fn static_shape(id: u32, element: &ShapeElement) -> Elem {
    let mut sp_pr = Elem::new("p:spPr")
        .child(xfrm(
            element.left,
            element.top,
            element.width,
            element.height,
        ))
        .child(prst_geom(element.shape.preset()));
    if element.shape == ShapeKind::Line {
        sp_pr = sp_pr.child(Elem::new("a:noFill")).child(
            Elem::new("a:ln")
                .attr("w", "25400")
                .child(solid_fill(element.fill.as_deref().unwrap_or("000000"))),
        );
    } else if element.no_fill {
        sp_pr = sp_pr.child(Elem::new("a:noFill"));
    } else {
        sp_pr = sp_pr.child(solid_fill(element.fill.as_deref().unwrap_or("D9D9D9")));
    }

    Elem::new("p:sp")
        .child(
            Elem::new("p:nvSpPr")
                .child(
                    Elem::new("p:cNvPr")
                        .attr("id", id.to_string())
                        .attr("name", format!("Shape {id}")),
                )
                .child(Elem::new("p:cNvSpPr"))
                .child(Elem::new("p:nvPr")),
        )
        .child(sp_pr)
}

/// A text shape on a slide, bound to a layout text slot.
pub fn text_shape(
    id: u32,
    name: &str,
    slot: &SlotElement,
    paragraphs: &[Paragraph],
    font: &str,
) -> Elem {
    let mut sp_pr = Elem::new("p:spPr")
        .child(xfrm(slot.left, slot.top, slot.width, slot.height))
        .child(prst_geom("rect"));
    sp_pr = match &slot.fill {
        Some(fill) => sp_pr.child(solid_fill(fill)),
        None => sp_pr.child(Elem::new("a:noFill")),
    };

    let wrap = if slot.wrap.unwrap_or(true) {
        "square"
    } else {
        "none"
    };
    let body_pr = Elem::new("a:bodyPr")
        .attr("wrap", wrap)
        .attr("anchor", slot.anchor.attr());

    let mut tx_body = Elem::new("p:txBody")
        .child(body_pr)
        .child(Elem::new("a:lstStyle"));
    for paragraph in paragraphs {
        tx_body = tx_body.child(text_paragraph(paragraph, slot, font));
    }
    if paragraphs.is_empty() {
        tx_body = tx_body.child(Elem::new("a:p"));
    }

    Elem::new("p:sp")
        .child(
            Elem::new("p:nvSpPr")
                .child(
                    Elem::new("p:cNvPr")
                        .attr("id", id.to_string())
                        .attr("name", name),
                )
                .child(Elem::new("p:cNvSpPr").attr("txBox", "1"))
                .child(Elem::new("p:nvPr")),
        )
        .child(sp_pr)
        .child(tx_body)
}

fn text_paragraph(paragraph: &Paragraph, slot: &SlotElement, font: &str) -> Elem {
    let mut p_pr = Elem::new("a:pPr")
        .attr("lvl", paragraph.level.to_string())
        .attr("algn", slot.align.attr());
    if paragraph.bullet {
        p_pr = p_pr
            .child(Elem::new("a:buFont").attr("typeface", "Arial"))
            .child(Elem::new("a:buChar").attr("char", "•"));
    } else {
        p_pr = p_pr.child(Elem::new("a:buNone"));
    }

    let mut r_pr = Elem::new("a:rPr").attr("lang", "en-US").attr("dirty", "0");
    if let Some(size) = slot.text_size {
        r_pr = r_pr.attr("sz", (size * 100).to_string());
    }
    if slot.bold.unwrap_or(false) {
        r_pr = r_pr.attr("b", "1");
    }
    if slot.italic.unwrap_or(false) {
        r_pr = r_pr.attr("i", "1");
    }
    if let Some(color) = &slot.color {
        r_pr = r_pr.child(solid_fill(color));
    }
    r_pr = r_pr.child(Elem::new("a:latin").attr("typeface", font));

    Elem::new("a:p").child(p_pr).child(
        Elem::new("a:r")
            .child(r_pr)
            .child(Elem::new("a:t").text(paragraph.text.clone())),
    )
}

/// A picture shape on a slide, bound to a layout picture slot.
pub fn picture_shape(id: u32, name: &str, slot: &SlotElement, embed: &str) -> Elem {
    Elem::new("p:pic")
        .child(
            Elem::new("p:nvPicPr")
                .child(
                    Elem::new("p:cNvPr")
                        .attr("id", id.to_string())
                        .attr("name", name),
                )
                .child(Elem::new("p:cNvPicPr"))
                .child(Elem::new("p:nvPr")),
        )
        .child(
            Elem::new("p:blipFill")
                .child(Elem::new("a:blip").attr("r:embed", embed))
                .child(Elem::new("a:stretch").child(Elem::new("a:fillRect"))),
        )
        .child(
            Elem::new("p:spPr")
                .child(xfrm(slot.left, slot.top, slot.width, slot.height))
                .child(prst_geom("rect")),
        )
}
