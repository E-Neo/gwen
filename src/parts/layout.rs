//! `ppt/slideLayouts/slideLayoutN.xml`. A layout carries only its decorative
//! (static) shapes; slide content is placed on the slide itself using the
//! slot geometry from config.

use crate::config::{Element, Layout};
use crate::ooxml::Elem;
use crate::parts::{IdGen, sp_tree, static_shape};

/// Build the layout part for `key` at package URI `uri`.
pub fn build(name: &str, layout: &Layout) -> Vec<u8> {
    let mut ids = IdGen::new();
    let shapes = layout
        .elements
        .iter()
        .filter_map(|element| match element {
            Element::Shape(shape) => Some(static_shape(ids.take(), shape)),
            Element::Slot(_) => None,
        })
        .collect();

    Elem::new("p:sldLayout")
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
        .attr("preserve", "1")
        .child(
            Elem::new("p:cSld")
                .attr("name", name)
                .child(sp_tree(shapes)),
        )
        .child(Elem::new("p:clrMapOvr").child(Elem::new("a:masterClrMapping")))
        .to_xml()
}

/// The package URI for layout number `n` (1-based).
pub fn uri(n: usize) -> String {
    format!("ppt/slideLayouts/slideLayout{n}.xml")
}
