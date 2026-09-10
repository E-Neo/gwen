//! Notes slide and notes master parts.

use crate::ooxml::Elem;
use crate::parts::{IdGen, sp_tree, xfrm};

pub const MASTER_URI: &str = "ppt/notesMasters/notesMaster1.xml";

pub fn slide_uri(n: usize) -> String {
    format!("ppt/notesSlides/notesSlide{n}.xml")
}

/// A notes slide carrying the speaker text.
pub fn build_slide(text: &str) -> Vec<u8> {
    let mut ids = IdGen::new();
    let shape = plain_text_box(ids.take(), text);
    Elem::new("p:notesSlide")
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
        .child(Elem::new("p:cSld").child(sp_tree(vec![shape])))
        .child(Elem::new("p:clrMapOvr").child(Elem::new("a:masterClrMapping")))
        .to_xml()
}

/// The shared notes master: the standard slide-image and body placeholders.
pub fn build_master() -> Vec<u8> {
    let sld_img = placeholder(2, "Slide Image Placeholder 1", "sldImg", None);
    let body = placeholder(3, "Notes Placeholder 2", "body", Some("1"));
    Elem::new("p:notesMaster")
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
        .child(Elem::new("p:cSld").child(sp_tree(vec![sld_img, body])))
        .child(
            Elem::new("a:clrMap")
                .attr("bg1", "lt1")
                .attr("tx1", "dk1")
                .attr("bg2", "lt2")
                .attr("tx2", "dk2")
                .attr("accent1", "accent1")
                .attr("accent2", "accent2")
                .attr("accent3", "accent3")
                .attr("accent4", "accent4")
                .attr("accent5", "accent5")
                .attr("accent6", "accent6")
                .attr("hlink", "hlink")
                .attr("folHlink", "folHlink"),
        )
        .to_xml()
}

fn placeholder(id: u32, name: &str, ph_type: &str, idx: Option<&str>) -> Elem {
    let mut ph = Elem::new("p:ph").attr("type", ph_type);
    if let Some(idx) = idx {
        ph = ph.attr("idx", idx);
    }
    Elem::new("p:sp")
        .child(
            Elem::new("p:nvSpPr")
                .child(
                    Elem::new("p:cNvPr")
                        .attr("id", id.to_string())
                        .attr("name", name),
                )
                .child(Elem::new("p:cNvSpPr"))
                .child(Elem::new("p:nvPr").child(ph)),
        )
        .child(Elem::new("p:spPr"))
        .child(
            Elem::new("p:txBody")
                .child(Elem::new("a:bodyPr"))
                .child(Elem::new("a:lstStyle"))
                .child(Elem::new("a:p")),
        )
}

/// A simple text box used for the notes body (geometry from the notes master).
fn plain_text_box(id: u32, text: &str) -> Elem {
    let mut body = Elem::new("p:txBody")
        .child(Elem::new("a:bodyPr"))
        .child(Elem::new("a:lstStyle"));
    for line in text.lines() {
        body = body.child(
            Elem::new("a:p").child(
                Elem::new("a:r")
                    .child(Elem::new("a:rPr").attr("lang", "en-US"))
                    .child(Elem::new("a:t").text(line.to_string())),
            ),
        );
    }
    if text.is_empty() {
        body = body.child(Elem::new("a:p"));
    }
    Elem::new("p:sp")
        .child(
            Elem::new("p:nvSpPr")
                .child(
                    Elem::new("p:cNvPr")
                        .attr("id", id.to_string())
                        .attr("name", "Notes"),
                )
                .child(Elem::new("p:cNvSpPr").attr("txBox", "1"))
                .child(Elem::new("p:nvPr")),
        )
        .child(Elem::new("p:spPr").child(xfrm(685800, 1143000, 5486400, 3086100)))
        .child(body)
}
