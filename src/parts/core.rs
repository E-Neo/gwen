//! `docProps/core.xml`. Timestamps are fixed so output is deterministic.

use crate::ooxml::Elem;

pub const URI: &str = "docProps/core.xml";

const CREATED: &str = "2000-01-01T00:00:00Z";

pub fn build(title: &str) -> Vec<u8> {
    Elem::new("cp:coreProperties")
        .attr(
            "xmlns:cp",
            "http://schemas.openxmlformats.org/package/2006/metadata/core-properties",
        )
        .attr("xmlns:dc", "http://purl.org/dc/elements/1.1/")
        .attr("xmlns:dcterms", "http://purl.org/dc/terms/")
        .attr("xmlns:dcmitype", "http://purl.org/dc/dcmitype/")
        .attr("xmlns:xsi", "http://www.w3.org/2001/XMLSchema-instance")
        .child(Elem::new("dc:title").text(title.to_string()))
        .child(Elem::new("cp:revision").text("1"))
        .child(
            Elem::new("dcterms:created")
                .attr("xsi:type", "dcterms:W3CDTF")
                .text(CREATED),
        )
        .child(
            Elem::new("dcterms:modified")
                .attr("xsi:type", "dcterms:W3CDTF")
                .text(CREATED),
        )
        .to_xml()
}
