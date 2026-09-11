//! Presentation-level support parts that PowerPoint expects in every package:
//! `presProps`, `viewProps`, `tableStyles` and `docProps/app.xml`.

use crate::ooxml::Elem;

pub const PRES_PROPS_URI: &str = "ppt/presProps.xml";
pub const VIEW_PROPS_URI: &str = "ppt/viewProps.xml";
pub const TABLE_STYLES_URI: &str = "ppt/tableStyles.xml";
pub const APP_URI: &str = "docProps/app.xml";

const DEFAULT_TABLE_STYLE: &str = "{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}";

pub fn pres_props() -> Vec<u8> {
    Elem::new("p:presentationPr")
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
            Elem::new("p:clrMru")
                .child(Elem::new("a:srgbClr").attr("val", "000000"))
                .child(Elem::new("a:srgbClr").attr("val", "FFFFFF")),
        )
        .to_xml()
}

pub fn view_props() -> Vec<u8> {
    Elem::new("p:viewPr")
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
            Elem::new("p:normalViewPr")
                .child(
                    Elem::new("p:restoredLeft")
                        .attr("sz", "15620")
                        .attr("autoAdjust", "0"),
                )
                .child(Elem::new("p:restoredTop").attr("sz", "94660")),
        )
        .child(
            Elem::new("p:slideViewPr").child(
                Elem::new("p:cSldViewPr")
                    .child(view_scale("100", "100"))
                    .child(Elem::new("p:guideLst")),
            ),
        )
        .child(Elem::new("p:outlineViewPr").child(view_scale("33", "100")))
        .child(Elem::new("p:notesTextViewPr").child(view_scale("100", "100")))
        .child(
            Elem::new("p:gridSpacing")
                .attr("cx", "72008")
                .attr("cy", "72008"),
        )
        .to_xml()
}

fn view_scale(sx: &str, sy: &str) -> Elem {
    Elem::new("p:cViewPr").attr("varScale", "1").child(
        Elem::new("p:scale")
            .child(Elem::new("a:sx").attr("n", sx).attr("d", "100"))
            .child(Elem::new("a:sy").attr("n", sy).attr("d", "100")),
    )
}

/// The default table style list. We generate no tables yet, but PowerPoint
/// requires the part and a `def` style that resolves.
pub fn table_styles() -> Vec<u8> {
    Elem::new("a:tblStyleLst")
        .attr(
            "xmlns:a",
            "http://schemas.openxmlformats.org/drawingml/2006/main",
        )
        .attr("def", DEFAULT_TABLE_STYLE)
        .child(
            Elem::new("a:tblStyle")
                .attr("styleId", DEFAULT_TABLE_STYLE)
                .attr("styleName", "Medium Style 2 - Accent 1")
                .child(
                    Elem::new("a:wholeTbl")
                        .child(
                            Elem::new("a:tcTxStyle").child(
                                Elem::new("a:fontRef")
                                    .attr("idx", "minor")
                                    .child(Elem::new("a:schemeClr").attr("val", "dk1")),
                            ),
                        )
                        .child(
                            Elem::new("a:tcStyle").child(
                                Elem::new("a:fill").child(
                                    Elem::new("a:solidFill").child(
                                        Elem::new("a:schemeClr")
                                            .attr("val", "accent1")
                                            .child(Elem::new("a:tint").attr("val", "20000")),
                                    ),
                                ),
                            ),
                        ),
                ),
        )
        .to_xml()
}

pub fn app_props(slides: usize) -> Vec<u8> {
    Elem::new("Properties")
        .attr(
            "xmlns",
            "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties",
        )
        .attr(
            "xmlns:vt",
            "http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes",
        )
        .child(Elem::new("Application").text("Microsoft Office PowerPoint"))
        .child(Elem::new("PresentationFormat").text("On-screen Show (16:9)"))
        .child(Elem::new("Slides").text(slides.to_string()))
        .child(Elem::new("Notes").text("0"))
        .child(Elem::new("Company").text(""))
        .child(Elem::new("AppVersion").text("16.0000"))
        .to_xml()
}
