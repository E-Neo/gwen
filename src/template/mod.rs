use std::collections::HashMap;

use crate::error::AppResult;
use crate::opc::Package;

/// Minimal, known-good presentation template. The parts below were authored
/// from a python-pptx-generated baseline (validated to open cleanly in
/// LibreOffice and PowerPoint). They live as plain XML source so the template
/// is auditable and requires no Python tooling at build or test time.
mod xml {
    pub const CONTENT_TYPES: &str = include_str!("xml/[Content_Types].xml");
    pub const APP: &str = include_str!("xml/docProps/app.xml");
    pub const CORE: &str = include_str!("xml/docProps/core.xml");
    pub const PRESENTATION: &str = include_str!("xml/ppt/presentation.xml");
    pub const PRES_PROPS: &str = include_str!("xml/ppt/presProps.xml");
    pub const TABLE_STYLES: &str = include_str!("xml/ppt/tableStyles.xml");
    pub const VIEW_PROPS: &str = include_str!("xml/ppt/viewProps.xml");
    pub const MASTER: &str = include_str!("xml/ppt/slideMasters/slideMaster1.xml");
    pub const SLIDE: &str = include_str!("xml/ppt/slides/slide1.xml");
    pub const THEME: &str = include_str!("xml/ppt/theme/theme1.xml");

    pub fn slide_layout(num: u32) -> &'static str {
        match num {
            1 => include_str!("xml/ppt/slideLayouts/slideLayout1.xml"),
            2 => include_str!("xml/ppt/slideLayouts/slideLayout2.xml"),
            3 => include_str!("xml/ppt/slideLayouts/slideLayout3.xml"),
            4 => include_str!("xml/ppt/slideLayouts/slideLayout4.xml"),
            5 => include_str!("xml/ppt/slideLayouts/slideLayout5.xml"),
            6 => include_str!("xml/ppt/slideLayouts/slideLayout6.xml"),
            7 => include_str!("xml/ppt/slideLayouts/slideLayout7.xml"),
            8 => include_str!("xml/ppt/slideLayouts/slideLayout8.xml"),
            9 => include_str!("xml/ppt/slideLayouts/slideLayout9.xml"),
            10 => include_str!("xml/ppt/slideLayouts/slideLayout10.xml"),
            11 => include_str!("xml/ppt/slideLayouts/slideLayout11.xml"),
            _ => panic!("no template for slide layout {num}"),
        }
    }
}

const SLIDE_LAYOUT_COUNT: u32 = 11;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlideSize {
    /// 16:9 widescreen (12192000 x 6858000 EMU)
    Wide,
    /// 4:3 standard (9144000 x 6858000 EMU)
    Standard,
}

impl SlideSize {
    pub fn parse(s: &str) -> AppResult<Self> {
        match s {
            "16:9" | "wide" | "widescreen" => Ok(SlideSize::Wide),
            "4:3" | "standard" => Ok(SlideSize::Standard),
            other => Err(crate::error::AppError::InvalidValue(format!(
                "invalid slide size '{other}' (expected '16:9' or '4:3')"
            ))),
        }
    }
}

/// Build a new empty presentation package at the requested slide size.
pub fn build_default_package(size: SlideSize) -> AppResult<Package> {
    let mut parts: HashMap<String, Vec<u8>> = HashMap::new();

    let mut insert = |uri: &str, content: &str| {
        parts.insert(uri.to_string(), content.as_bytes().to_vec());
    };

    insert("[Content_Types].xml", xml::CONTENT_TYPES);
    insert("docProps/app.xml", xml::APP);
    insert("docProps/core.xml", xml::CORE);
    insert("ppt/presentation.xml", xml::PRESENTATION);
    insert("ppt/presProps.xml", xml::PRES_PROPS);
    insert("ppt/tableStyles.xml", xml::TABLE_STYLES);
    insert("ppt/viewProps.xml", xml::VIEW_PROPS);
    insert("ppt/slideMasters/slideMaster1.xml", xml::MASTER);
    insert("ppt/slides/slide1.xml", xml::SLIDE);
    insert("ppt/theme/theme1.xml", xml::THEME);
    for n in 1..=SLIDE_LAYOUT_COUNT {
        insert(
            &format!("ppt/slideLayouts/slideLayout{n}.xml"),
            xml::slide_layout(n),
        );
    }

    let mut relationships: HashMap<String, HashMap<String, crate::opc::Relationship>> =
        HashMap::new();
    let mut insert_rels = |uri: &str, rels_xml: &str| -> AppResult<()> {
        let rels = crate::opc::package::parse_rels_xml(rels_xml.as_bytes())?;
        relationships.insert(uri.to_string(), rels);
        Ok(())
    };

    insert_rels("", include_str!("xml/_rels/.rels"))?;
    insert_rels(
        "ppt/presentation.xml",
        include_str!("xml/ppt/_rels/presentation.xml.rels"),
    )?;
    insert_rels(
        "ppt/slideMasters/slideMaster1.xml",
        include_str!("xml/ppt/slideMasters/_rels/slideMaster1.xml.rels"),
    )?;
    insert_rels(
        "ppt/slides/slide1.xml",
        include_str!("xml/ppt/slides/_rels/slide1.xml.rels"),
    )?;
    for n in 1..=SLIDE_LAYOUT_COUNT {
        insert_rels(
            &format!("ppt/slideLayouts/slideLayout{n}.xml"),
            match n {
                1 => include_str!("xml/ppt/slideLayouts/_rels/slideLayout1.xml.rels"),
                2 => include_str!("xml/ppt/slideLayouts/_rels/slideLayout2.xml.rels"),
                3 => include_str!("xml/ppt/slideLayouts/_rels/slideLayout3.xml.rels"),
                4 => include_str!("xml/ppt/slideLayouts/_rels/slideLayout4.xml.rels"),
                5 => include_str!("xml/ppt/slideLayouts/_rels/slideLayout5.xml.rels"),
                6 => include_str!("xml/ppt/slideLayouts/_rels/slideLayout6.xml.rels"),
                7 => include_str!("xml/ppt/slideLayouts/_rels/slideLayout7.xml.rels"),
                8 => include_str!("xml/ppt/slideLayouts/_rels/slideLayout8.xml.rels"),
                9 => include_str!("xml/ppt/slideLayouts/_rels/slideLayout9.xml.rels"),
                10 => include_str!("xml/ppt/slideLayouts/_rels/slideLayout10.xml.rels"),
                _ => include_str!("xml/ppt/slideLayouts/_rels/slideLayout11.xml.rels"),
            },
        )?;
    }

    let mut pkg = Package::from_parts(parts, relationships);

    if size == SlideSize::Wide {
        let pres_data = pkg
            .get_part("ppt/presentation.xml")
            .ok_or_else(|| {
                crate::error::AppError::PartNotFound("ppt/presentation.xml".to_string())
            })?
            .to_vec();
        let text = String::from_utf8_lossy(&pres_data);
        let old = "<p:sldSz cx=\"9144000\" cy=\"6858000\" type=\"screen4x3\"/>";
        let new = "<p:sldSz cx=\"12192000\" cy=\"6858000\" type=\"screen16x9\"/>";
        if !text.contains(old) {
            return Err(crate::error::AppError::InvalidValue(
                "unexpected sldSz in template".to_string(),
            ));
        }
        let updated = text.replacen(old, new, 1);
        pkg.set_part("ppt/presentation.xml", updated.into_bytes());
    }

    Ok(pkg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_all_rels_resolve(pkg: &Package) {
        for (source_uri, rels) in pkg.relationship_sources() {
            for (r_id, rel) in rels {
                if rel.target_mode.as_deref() == Some("External") {
                    continue;
                }
                let target = pkg.resolve_relationship_target(source_uri, rel);
                assert!(
                    target.is_some(),
                    "relationship {r_id} of {source_uri} targets missing part {}",
                    rel.target
                );
            }
        }
    }

    #[test]
    fn template_package_is_consistent() {
        for size in [SlideSize::Wide, SlideSize::Standard] {
            let pkg = build_default_package(size).unwrap();
            assert_all_rels_resolve(&pkg);
            assert!(pkg.get_part("[Content_Types].xml").is_some());
            assert!(pkg.get_part("ppt/presentation.xml").is_some());
            assert!(pkg.get_part("ppt/slideMasters/slideMaster1.xml").is_some());
            assert!(pkg.get_part("ppt/slides/slide1.xml").is_some());
            assert!(pkg.get_part("ppt/theme/theme1.xml").is_some());
        }
    }

    #[test]
    fn wide_size_is_applied() {
        let pkg = build_default_package(SlideSize::Wide).unwrap();
        let pres = String::from_utf8_lossy(pkg.get_part("ppt/presentation.xml").unwrap());
        assert!(pres.contains("cx=\"12192000\""));
        assert!(pres.contains("cy=\"6858000\""));
    }

    #[test]
    fn template_parses_with_query_model() {
        let pkg = build_default_package(SlideSize::Wide).unwrap();
        let pres = pkg.get_part("ppt/presentation.xml").unwrap();
        let rels = pkg.get_rels("ppt/presentation.xml").unwrap();
        let mut p = crate::model::presentation::Presentation::parse(pres).unwrap();
        p.slide_uris = p.resolve_slide_uris(rels);
        assert_eq!(p.slide_uris.len(), 1);
        assert!(p.slide_uris[0].ends_with("slide1.xml"));

        let empty_map = HashMap::new();
        let slide = pkg.get_part(&p.slide_uris[0]).unwrap();
        crate::model::slide::parse_slide_shapes(slide, &empty_map).unwrap();
    }
}
