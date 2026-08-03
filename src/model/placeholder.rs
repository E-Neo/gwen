use std::collections::HashMap;

use crate::dto::*;
use crate::error::AppResult;
use crate::opc::Package;

/// OOXML placeholder identity: an absent `type` defaults to a title
/// placeholder, an absent `idx` defaults to 0.
fn placeholder_key(ph: &PlaceholderFormatDto) -> (PlaceholderType, i32) {
    (ph.ph_type.clone().unwrap_or(PlaceholderType::Title), ph.idx)
}

/// Ancestor parts whose placeholder styling a part inherits, nearest first:
/// slide → slideLayout → slideMaster; layout → slideMaster; notes → notesMaster.
fn ancestor_parts(pkg: &Package, part_uri: &str) -> Vec<String> {
    let mut out = Vec::new();
    let rel_target = |rel_type: &str| -> Option<String> {
        pkg.get_rels(part_uri)?.values().find_map(|rel| {
            if rel.rel_type.contains(rel_type) {
                pkg.resolve_relationship_target(part_uri, rel)
            } else {
                None
            }
        })
    };

    if part_uri.contains("notesSlides/") {
        if let Some(m) = rel_target("notesMaster") {
            out.push(m);
        }
    } else if part_uri.contains("slides/") {
        if let Some(layout) = rel_target("slideLayout") {
            out.push(layout.clone());
            if let Some(master) = pkg.get_rels(&layout).and_then(|rels| {
                rels.values().find_map(|rel| {
                    if rel.rel_type.contains("slideMaster") {
                        pkg.resolve_relationship_target(&layout, rel)
                    } else {
                        None
                    }
                })
            }) {
                out.push(master);
            }
        }
    } else if part_uri.contains("slideLayouts/")
        && let Some(m) = rel_target("slideMaster")
    {
        out.push(m);
    }
    out
}

fn merge_geometry(target: &mut ShapeDto, source: &ShapeDto) {
    if target.left.is_none() {
        target.left = source.left;
    }
    if target.top.is_none() {
        target.top = source.top;
    }
    if target.width.is_none() {
        target.width = source.width;
    }
    if target.height.is_none() {
        target.height = source.height;
    }
    if target.rotation.is_none() {
        target.rotation = source.rotation;
    }
}

fn merge_fill_outline(target: &mut ShapeDto, source: &ShapeDto) {
    if target.fill.is_none() {
        target.fill = source.fill.clone();
    }
    if target.outline.is_none() {
        target.outline = source.outline.clone();
    }
}

fn merge_text_style(target: &mut ShapeDto, source: &ShapeDto) {
    let Some(src) = &source.text_frame else {
        return;
    };
    let dst = target.text_frame.get_or_insert_with(|| TextFrameDto {
        paragraphs: Vec::new(),
        auto_size: None,
        word_wrap: None,
        vertical_anchor: None,
        margin_left: None,
        margin_right: None,
        margin_top: None,
        margin_bottom: None,
        default_paragraph_style: None,
    });
    if dst.auto_size.is_none() {
        dst.auto_size = src.auto_size.clone();
    }
    if dst.word_wrap.is_none() {
        dst.word_wrap = src.word_wrap;
    }
    if dst.vertical_anchor.is_none() {
        dst.vertical_anchor = src.vertical_anchor.clone();
    }
    if dst.margin_left.is_none() {
        dst.margin_left = src.margin_left;
    }
    if dst.margin_right.is_none() {
        dst.margin_right = src.margin_right;
    }
    if dst.margin_top.is_none() {
        dst.margin_top = src.margin_top;
    }
    if dst.margin_bottom.is_none() {
        dst.margin_bottom = src.margin_bottom;
    }
    if dst.default_paragraph_style.is_none() {
        dst.default_paragraph_style = src.default_paragraph_style.clone();
    }
}

fn needs_resolution(shape: &ShapeDto) -> bool {
    shape.fill.is_none()
        || shape.outline.is_none()
        || shape.left.is_none()
        || shape.top.is_none()
        || shape.width.is_none()
        || shape.height.is_none()
        || shape
            .text_frame
            .as_ref()
            .is_some_and(|tf| tf.default_paragraph_style.is_none())
}

fn source_map<'a>(
    shapes: &'a [ShapeDto],
    map: &mut HashMap<(PlaceholderType, i32), &'a ShapeDto>,
    by_type: &mut HashMap<PlaceholderType, (&'a ShapeDto, usize)>,
) {
    for shape in shapes {
        if shape.is_placeholder
            && let Some(ph) = &shape.placeholder_format
        {
            let key = placeholder_key(ph);
            map.entry(key.clone()).or_insert(shape);
            let entry = by_type.entry(key.0.clone()).or_insert((shape, 0));
            entry.0 = shape;
            entry.1 += 1;
        }
        if let Some(children) = &shape.shapes {
            source_map(children, map, by_type);
        }
    }
}

fn resolve_level(
    shapes: &mut [ShapeDto],
    map: &HashMap<(PlaceholderType, i32), &ShapeDto>,
    by_type: &HashMap<PlaceholderType, (&ShapeDto, usize)>,
) {
    for shape in shapes {
        if shape.is_placeholder
            && let Some(ph) = &shape.placeholder_format
        {
            if !needs_resolution(shape) {
                continue;
            }
            let key = placeholder_key(ph);
            let source = map
                .get(&key)
                .copied()
                .or_else(|| match by_type.get(&key.0) {
                    Some((source, 1)) => Some(*source),
                    _ => None,
                });
            if let Some(source) = source {
                merge_geometry(shape, source);
                merge_fill_outline(shape, source);
                merge_text_style(shape, source);
            }
        }
        if let Some(children) = shape.shapes.as_mut() {
            resolve_level(children, map, by_type);
        }
    }
}

fn any_needs_resolution(shapes: &[ShapeDto]) -> bool {
    shapes
        .iter()
        .any(|s| needs_resolution(s) || s.shapes.as_ref().is_some_and(|c| any_needs_resolution(c)))
}

/// Fill inherited placeholder styling (geometry, fill, outline, text defaults)
/// from the nearest ancestor part that defines it, walking
/// slide → slideLayout → slideMaster (notes → notesMaster, layout → slideMaster).
///
/// This is a read-only augmentation: the underlying XML is never modified, so
/// layout/master restyling keeps working.
pub fn resolve_placeholder_properties(
    pkg: &Package,
    part_uri: &str,
    shapes: &mut [ShapeDto],
) -> AppResult<()> {
    let mut any = any_needs_resolution(shapes);
    if any {
        for ancestor in ancestor_parts(pkg, part_uri) {
            let data = match pkg.get_part(&ancestor) {
                Some(d) => d,
                None => continue,
            };
            let ancestor_shapes = crate::model::slide::parse_slide_shapes(data, &HashMap::new())?;
            let mut map = HashMap::new();
            let mut by_type = HashMap::new();
            source_map(&ancestor_shapes, &mut map, &mut by_type);
            resolve_level(shapes, &map, &by_type);
            any = any_needs_resolution(shapes);
            if !any {
                break;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opc::Relationship;

    fn rel(target: &str, rel_type: &str) -> HashMap<String, Relationship> {
        let mut m = HashMap::new();
        m.insert(
            "rId1".to_string(),
            Relationship {
                id: "rId1".to_string(),
                target: target.to_string(),
                target_mode: None,
                rel_type: rel_type.to_string(),
            },
        );
        m
    }

    fn shape(id: u32, ph: Option<&str>, sp_pr: &str, tx_body: &str) -> String {
        let ph_tag = ph
            .map(|p| format!("<p:nvPr><p:ph {p}/></p:nvPr>"))
            .unwrap_or_else(|| "<p:nvPr/>".to_string());
        format!(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="s{id}"/><p:cNvSpPr/>{ph_tag}</p:nvSpPr><p:spPr>{sp_pr}</p:spPr>{tx_body}</p:sp>"#
        )
    }

    const HDR: &str = r#"<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>"#;
    const FTR: &str = r#"</p:spTree></p:cSld></p:sld>"#;

    fn empty_txbody() -> String {
        "<p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang=\"en-US\"/></a:p></p:txBody>"
            .into()
    }

    fn rich_placeholder_xml() -> String {
        let sp = shape(
            10,
            Some(r#"type="ctrTitle""#),
            r#"<a:xfrm><a:off x="100" y="200"/><a:ext cx="300" cy="400"/></a:xfrm><a:prstGeom prst="rect"/><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill><a:ln w="12700"><a:solidFill><a:srgbClr val="00FF00"/></a:solidFill></a:ln>"#,
            r#"<p:txBody><a:bodyPr anchor="ctr" lIns="10" tIns="20" rIns="30" bIns="40"/><a:lstStyle><a:lvl1pPr algn="ctr"><a:defRPr sz="3200" b="1"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill><a:latin typeface="Calibri"/></a:defRPr></a:lvl1pPr></a:lstStyle><a:p><a:endParaRPr lang="en-US"/></a:p></p:txBody>"#,
        );
        format!("{HDR}{sp}{FTR}")
    }

    fn bare_slide_xml() -> String {
        let sp = shape(2, Some(r#"type="ctrTitle""#), "", &empty_txbody());
        format!("{HDR}{sp}{FTR}")
    }

    fn slide_with_layout() -> (Package, Vec<ShapeDto>) {
        let layout = rich_placeholder_xml();
        let slide = bare_slide_xml();
        let mut parts = HashMap::new();
        parts.insert("ppt/slides/slide1.xml".to_string(), slide.into_bytes());
        parts.insert(
            "ppt/slideLayouts/slideLayout1.xml".to_string(),
            layout.into_bytes(),
        );
        let mut relationships = HashMap::new();
        relationships.insert(
            "ppt/slides/slide1.xml".to_string(),
            rel(
                "../slideLayouts/slideLayout1.xml",
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout",
            ),
        );
        let pkg = Package::from_parts(parts, relationships);
        let shapes = crate::model::slide::parse_slide_shapes(
            pkg.get_part("ppt/slides/slide1.xml").unwrap(),
            &HashMap::new(),
        )
        .unwrap();
        (pkg, shapes)
    }

    #[test]
    fn inherits_geometry_fill_outline_and_text_style() {
        let (pkg, mut shapes) = slide_with_layout();
        resolve_placeholder_properties(&pkg, "ppt/slides/slide1.xml", &mut shapes).unwrap();

        let s = &shapes[0];
        assert_eq!(s.left, Some(100));
        assert_eq!(s.top, Some(200));
        assert_eq!(s.width, Some(300));
        assert_eq!(s.height, Some(400));

        let fill = s.fill.as_ref().unwrap();
        assert_eq!(fill.fill_type, Some(FillType::Solid));
        assert_eq!(fill.color.as_ref().unwrap().rgb.as_deref(), Some("FF0000"));

        let outline = s.outline.as_ref().unwrap();
        assert_eq!(outline.width, Some(12700));
        assert_eq!(
            outline
                .fill
                .as_ref()
                .unwrap()
                .color
                .as_ref()
                .unwrap()
                .rgb
                .as_deref(),
            Some("00FF00")
        );

        let tf = s.text_frame.as_ref().unwrap();
        assert_eq!(tf.vertical_anchor, Some(VerticalAnchor::Middle));
        assert_eq!(tf.margin_left, Some(10));
        assert_eq!(tf.margin_top, Some(20));
        let dps = tf.default_paragraph_style.as_ref().unwrap();
        assert_eq!(dps.alignment, Some(Alignment::Center));
        let font = dps.font.as_ref().unwrap();
        assert_eq!(font.name.as_deref(), Some("Calibri"));
        assert_eq!(font.size, Some(3200));
        assert_eq!(font.bold, Some(true));
        assert_eq!(
            font.color.as_ref().unwrap().theme_color.as_deref(),
            Some("tx1")
        );
    }

    #[test]
    fn slide_content_runs_are_untouched() {
        let (pkg, mut shapes) = slide_with_layout();
        resolve_placeholder_properties(&pkg, "ppt/slides/slide1.xml", &mut shapes).unwrap();
        // The bare slide placeholder has no typed runs; resolver must not fabricate content.
        assert!(
            shapes[0]
                .text_frame
                .as_ref()
                .unwrap()
                .paragraphs
                .iter()
                .all(|p| p.runs.is_empty())
        );
    }

    #[test]
    fn slide_explicit_fill_wins() {
        let layout = rich_placeholder_xml();
        let slide_sp = shape(
            2,
            Some(r#"type="ctrTitle""#),
            r#"<a:solidFill><a:srgbClr val="0000FF"/></a:solidFill>"#,
            &empty_txbody(),
        );
        let slide = format!("{HDR}{slide_sp}{FTR}");
        let mut parts = HashMap::new();
        parts.insert("ppt/slides/slide1.xml".to_string(), slide.into_bytes());
        parts.insert(
            "ppt/slideLayouts/slideLayout1.xml".to_string(),
            layout.into_bytes(),
        );
        let mut relationships = HashMap::new();
        relationships.insert(
            "ppt/slides/slide1.xml".to_string(),
            rel(
                "../slideLayouts/slideLayout1.xml",
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout",
            ),
        );
        let pkg = Package::from_parts(parts, relationships);
        let mut shapes = crate::model::slide::parse_slide_shapes(
            pkg.get_part("ppt/slides/slide1.xml").unwrap(),
            &HashMap::new(),
        )
        .unwrap();
        resolve_placeholder_properties(&pkg, "ppt/slides/slide1.xml", &mut shapes).unwrap();

        let s = &shapes[0];
        assert_eq!(
            s.fill
                .as_ref()
                .unwrap()
                .color
                .as_ref()
                .unwrap()
                .rgb
                .as_deref(),
            Some("0000FF")
        );
        assert_eq!(s.left, Some(100)); // geometry still inherited
    }

    #[test]
    fn resolves_through_layout_to_master() {
        // slide body idx=10, empty; layout body idx=10, empty spPr; master has geometry+fill.
        let master_sp = shape(
            50,
            Some(r#"type="body" idx="10""#),
            r#"<a:xfrm><a:off x="1000" y="2000"/><a:ext cx="3000" cy="4000"/></a:xfrm><a:solidFill><a:srgbClr val="00FF00"/></a:solidFill>"#,
            &empty_txbody(),
        );
        let master = format!("{HDR}{master_sp}{FTR}");
        let layout_sp = shape(20, Some(r#"type="body" idx="10""#), "", &empty_txbody());
        let layout = format!("{HDR}{layout_sp}{FTR}");
        let slide_sp = shape(2, Some(r#"type="body" idx="10""#), "", &empty_txbody());
        let slide = format!("{HDR}{slide_sp}{FTR}");

        let mut parts = HashMap::new();
        parts.insert("ppt/slides/slide1.xml".to_string(), slide.into_bytes());
        parts.insert(
            "ppt/slideLayouts/slideLayout1.xml".to_string(),
            layout.into_bytes(),
        );
        parts.insert(
            "ppt/slideMasters/slideMaster1.xml".to_string(),
            master.into_bytes(),
        );
        let mut relationships = HashMap::new();
        relationships.insert(
            "ppt/slides/slide1.xml".to_string(),
            rel(
                "../slideLayouts/slideLayout1.xml",
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout",
            ),
        );
        relationships.insert(
            "ppt/slideLayouts/slideLayout1.xml".to_string(),
            rel(
                "../slideMasters/slideMaster1.xml",
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster",
            ),
        );
        let pkg = Package::from_parts(parts, relationships);
        let mut shapes = crate::model::slide::parse_slide_shapes(
            pkg.get_part("ppt/slides/slide1.xml").unwrap(),
            &HashMap::new(),
        )
        .unwrap();
        resolve_placeholder_properties(&pkg, "ppt/slides/slide1.xml", &mut shapes).unwrap();

        let s = &shapes[0];
        assert_eq!(s.left, Some(1000));
        assert_eq!(s.height, Some(4000));
        assert_eq!(
            s.fill
                .as_ref()
                .unwrap()
                .color
                .as_ref()
                .unwrap()
                .rgb
                .as_deref(),
            Some("00FF00")
        );
    }

    #[test]
    fn non_matching_key_stays_empty() {
        // slide has ctrTitle, layout only has subTitle -> no geometry.
        let layout_sp = shape(
            20,
            Some(r#"type="subTitle" idx="1""#),
            r#"<a:xfrm><a:off x="5" y="6"/><a:ext cx="7" cy="8"/></a:xfrm>"#,
            &empty_txbody(),
        );
        let layout = format!("{HDR}{layout_sp}{FTR}");
        let slide = bare_slide_xml();
        let mut parts = HashMap::new();
        parts.insert("ppt/slides/slide1.xml".to_string(), slide.into_bytes());
        parts.insert(
            "ppt/slideLayouts/slideLayout1.xml".to_string(),
            layout.into_bytes(),
        );
        let mut relationships = HashMap::new();
        relationships.insert(
            "ppt/slides/slide1.xml".to_string(),
            rel(
                "../slideLayouts/slideLayout1.xml",
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout",
            ),
        );
        let pkg = Package::from_parts(parts, relationships);
        let mut shapes = crate::model::slide::parse_slide_shapes(
            pkg.get_part("ppt/slides/slide1.xml").unwrap(),
            &HashMap::new(),
        )
        .unwrap();
        resolve_placeholder_properties(&pkg, "ppt/slides/slide1.xml", &mut shapes).unwrap();
        assert!(shapes[0].left.is_none());
        assert!(shapes[0].fill.is_none());
    }

    #[test]
    fn fills_only_missing_geometry_fields() {
        let layout = rich_placeholder_xml();
        let slide_sp = shape(
            2,
            Some(r#"type="ctrTitle""#),
            r#"<a:xfrm><a:off x="99" y="88"/></a:xfrm>"#,
            &empty_txbody(),
        );
        let slide = format!("{HDR}{slide_sp}{FTR}");
        let mut parts = HashMap::new();
        parts.insert("ppt/slides/slide1.xml".to_string(), slide.into_bytes());
        parts.insert(
            "ppt/slideLayouts/slideLayout1.xml".to_string(),
            layout.into_bytes(),
        );
        let mut relationships = HashMap::new();
        relationships.insert(
            "ppt/slides/slide1.xml".to_string(),
            rel(
                "../slideLayouts/slideLayout1.xml",
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout",
            ),
        );
        let pkg = Package::from_parts(parts, relationships);
        let mut shapes = crate::model::slide::parse_slide_shapes(
            pkg.get_part("ppt/slides/slide1.xml").unwrap(),
            &HashMap::new(),
        )
        .unwrap();
        resolve_placeholder_properties(&pkg, "ppt/slides/slide1.xml", &mut shapes).unwrap();
        let s = &shapes[0];
        assert_eq!(s.left, Some(99)); // slide's explicit value kept
        assert_eq!(s.top, Some(88));
        assert_eq!(s.width, Some(300)); // inherited
        assert_eq!(s.height, Some(400));
    }

    #[test]
    fn type_and_idx_defaults_match() {
        // slide placeholder with no type/idx (title, idx 0) matches layout title idx 0.
        let layout_sp = shape(
            20,
            Some(r#"type="title""#),
            r#"<a:xfrm><a:off x="1" y="2"/><a:ext cx="3" cy="4"/></a:xfrm>"#,
            &empty_txbody(),
        );
        let layout = format!("{HDR}{layout_sp}{FTR}");
        let slide_sp = shape(2, Some(r#""#), "", &empty_txbody());
        let slide = format!("{HDR}{slide_sp}{FTR}");
        let mut parts = HashMap::new();
        parts.insert("ppt/slides/slide1.xml".to_string(), slide.into_bytes());
        parts.insert(
            "ppt/slideLayouts/slideLayout1.xml".to_string(),
            layout.into_bytes(),
        );
        let mut relationships = HashMap::new();
        relationships.insert(
            "ppt/slides/slide1.xml".to_string(),
            rel(
                "../slideLayouts/slideLayout1.xml",
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout",
            ),
        );
        let pkg = Package::from_parts(parts, relationships);
        let mut shapes = crate::model::slide::parse_slide_shapes(
            pkg.get_part("ppt/slides/slide1.xml").unwrap(),
            &HashMap::new(),
        )
        .unwrap();
        resolve_placeholder_properties(&pkg, "ppt/slides/slide1.xml", &mut shapes).unwrap();
        assert_eq!(shapes[0].left, Some(1));
        assert_eq!(shapes[0].width, Some(3));
    }

    #[test]
    fn resolves_placeholder_inside_group() {
        let layout_sp = shape(
            20,
            Some(r#"type="ctrTitle""#),
            r#"<a:xfrm><a:off x="1" y="2"/><a:ext cx="3" cy="4"/></a:xfrm>"#,
            &empty_txbody(),
        );
        let layout = format!("{HDR}{layout_sp}{FTR}");
        let inner = shape(3, Some(r#"type="ctrTitle""#), "", &empty_txbody());
        let slide = format!(
            r#"{HDR}<p:grpSp><p:nvGrpSpPr><p:cNvPr id="2" name="g"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>{inner}</p:grpSp>{FTR}"#
        );
        let mut parts = HashMap::new();
        parts.insert("ppt/slides/slide1.xml".to_string(), slide.into_bytes());
        parts.insert(
            "ppt/slideLayouts/slideLayout1.xml".to_string(),
            layout.into_bytes(),
        );
        let mut relationships = HashMap::new();
        relationships.insert(
            "ppt/slides/slide1.xml".to_string(),
            rel(
                "../slideLayouts/slideLayout1.xml",
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout",
            ),
        );
        let pkg = Package::from_parts(parts, relationships);
        let mut shapes = crate::model::slide::parse_slide_shapes(
            pkg.get_part("ppt/slides/slide1.xml").unwrap(),
            &HashMap::new(),
        )
        .unwrap();
        resolve_placeholder_properties(&pkg, "ppt/slides/slide1.xml", &mut shapes).unwrap();
        let child = &shapes[0].shapes.as_ref().unwrap()[0];
        assert_eq!(child.left, Some(1));
    }

    #[test]
    fn notes_resolves_from_notes_master_by_type() {
        // notes body placeholder has no idx (0); notesMaster body idx=3 (unique type).
        let master_sp = shape(
            30,
            Some(r#"type="body" idx="3""#),
            r#"<a:xfrm><a:off x="10" y="20"/><a:ext cx="30" cy="40"/></a:xfrm>"#,
            &empty_txbody(),
        );
        let master = format!("{HDR}{master_sp}{FTR}");
        let notes_sp = shape(3, Some(r#"type="body""#), "", &empty_txbody());
        let notes = format!("{HDR}{notes_sp}{FTR}");
        let mut parts = HashMap::new();
        parts.insert(
            "ppt/notesSlides/notesSlide1.xml".to_string(),
            notes.into_bytes(),
        );
        parts.insert(
            "ppt/notesMasters/notesMaster1.xml".to_string(),
            master.into_bytes(),
        );
        let mut relationships = HashMap::new();
        relationships.insert(
            "ppt/notesSlides/notesSlide1.xml".to_string(),
            rel(
                "../notesMasters/notesMaster1.xml",
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesMaster",
            ),
        );
        let pkg = Package::from_parts(parts, relationships);
        let mut shapes = crate::model::slide::parse_slide_shapes(
            pkg.get_part("ppt/notesSlides/notesSlide1.xml").unwrap(),
            &HashMap::new(),
        )
        .unwrap();
        resolve_placeholder_properties(&pkg, "ppt/notesSlides/notesSlide1.xml", &mut shapes)
            .unwrap();
        let s = &shapes[0];
        assert_eq!(s.left, Some(10));
        assert_eq!(s.height, Some(40));
    }
}
