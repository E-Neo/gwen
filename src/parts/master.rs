//! `ppt/slideMasters/slideMaster1.xml`.

use crate::ooxml::Elem;
use crate::parts::sp_tree;

pub const URI: &str = "ppt/slideMasters/slideMaster1.xml";

/// Build the single slide master. `layouts` pairs each layout's id with the
/// relationship id that targets it (both assigned by the engine).
pub fn build(name: &str, layouts: &[(u32, String)]) -> Vec<u8> {
    let layout_id_lst = Elem::new("p:sldLayoutIdLst").children(layouts.iter().map(|(id, rid)| {
        Elem::new("p:sldLayoutId")
            .attr("id", id.to_string())
            .attr("r:id", rid.clone())
    }));

    Elem::new("p:sldMaster")
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
        .child(
            Elem::new("p:cSld")
                .attr("name", name)
                .child(sp_tree(Vec::new())),
        )
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
        .child(layout_id_lst)
        .child(
            Elem::new("p:txStyles")
                .child(Elem::new("p:titleStyle"))
                .child(Elem::new("p:bodyStyle"))
                .child(Elem::new("p:otherStyle")),
        )
        .to_xml()
}
