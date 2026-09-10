//! `ppt/presentation.xml`.

use crate::ooxml::Elem;

pub const URI: &str = "ppt/presentation.xml";

/// Build the presentation part. `slides` pairs each slide's id with the
/// relationship id that targets it.
pub fn build(
    width: i64,
    height: i64,
    master_rid: &str,
    notes_master_rid: Option<&str>,
    slides: &[(u32, String)],
) -> Vec<u8> {
    let mut root = Elem::new("p:presentation")
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
            Elem::new("p:sldMasterIdLst").child(
                Elem::new("p:sldMasterId")
                    .attr("id", "2147483648")
                    .attr("r:id", master_rid),
            ),
        );

    if let Some(rid) = notes_master_rid {
        root = root.child(
            Elem::new("p:notesMasterIdLst").child(Elem::new("p:notesMasterId").attr("r:id", rid)),
        );
    }

    root = root.child(
        Elem::new("p:sldIdLst").children(slides.iter().map(|(id, rid)| {
            Elem::new("p:sldId")
                .attr("id", id.to_string())
                .attr("r:id", rid.clone())
        })),
    );

    root = root
        .child(
            Elem::new("p:sldSz")
                .attr("cx", width.to_string())
                .attr("cy", height.to_string()),
        )
        .child(
            Elem::new("p:notesSz")
                .attr("cx", "6858000")
                .attr("cy", "9144000"),
        )
        .child(
            Elem::new("p:defaultTextStyle").child(
                Elem::new("a:defPPr").child(
                    Elem::new("a:defRPr")
                        .attr("lang", "en-US")
                        .child(
                            Elem::new("a:solidFill")
                                .child(Elem::new("a:schemeClr").attr("val", "tx1")),
                        )
                        .child(Elem::new("a:latin").attr("typeface", "+mn-lt")),
                ),
            ),
        );
    root.to_xml()
}
