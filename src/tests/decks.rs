//! Programmatically generated test decks.
//!
//! The fixture decks are built here from shared XML skeletons instead of
//! committing binary `.pptx` files, so every part is auditable and new decks
//! are trivial to add. Each deck is written once per test process into a temp
//! directory, then consumed exactly like a real file.
//!
//! Structural notes:
//! - every slide references `slideLayout1`, which owns a `ctrTitle`
//!   placeholder whose geometry/fill/text defaults flow down to slides that
//!   use it (exercising `resolve_placeholder_properties`);
//! - the shared theme reproduces the python-pptx "Office" palette the CLI
//!   tests assert on (`accent1`/`accent2`, major font "Calibri");
//! - `notes_placeholder` carries a notesSlide whose `sldImg` placeholder has
//!   no `p:txBody`, while its notesMaster's does — the round-trip regression
//!   fixture for paragraph-less placeholder text frames.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const CT: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const RELS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

const REL_SLIDE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide";
const REL_MASTER: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster";
const REL_LAYOUT: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout";
const REL_NOTES_MASTER: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesMaster";
const REL_NOTES_SLIDE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide";
const REL_THEME: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme";
const REL_CHART: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart";
const REL_OFFICE_DOC: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";

/// Names of every deck this module can build, minus the `.pptx` suffix.
pub const DECKS: &[&str] = &[
    "template",
    "template_43",
    "two_slides",
    "placeholder",
    "table_chart",
    "notes_placeholder",
];

/// Path to a generated deck, materialized once per test process. Accepts the
/// name with or without the `.pptx` suffix.
pub fn deck(name: &str) -> PathBuf {
    let name = name.strip_suffix(".pptx").unwrap_or(name);
    static CACHE: OnceLock<Mutex<HashMap<&'static str, PathBuf>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = cache.lock().unwrap();
    if let Some(path) = cache.get(name) {
        return path.clone();
    }
    let dir = std::env::temp_dir().join(format!("gwen-decks-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.pptx"));
    write_deck(&path, &parts(name));
    cache.insert(Box::leak(name.to_string().into_boxed_str()), path.clone());
    path
}

fn write_deck(path: &PathBuf, parts: &[(String, Vec<u8>)]) {
    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
    for (name, data) in parts {
        zip.start_file(name, opts).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
}

fn parts(name: &str) -> Vec<(String, Vec<u8>)> {
    let mut p: Vec<(String, Vec<u8>)> = Vec::new();
    let (slide_xml, slide_size, notes): (Vec<String>, (i64, i64), bool) = match name {
        "template" => (vec![empty_slide()], (12_192_000, 6_858_000), false),
        "template_43" => (vec![empty_slide()], (9_144_000, 6_858_000), false),
        "two_slides" => (
            vec![textbox_slide("Alpha"), textbox_slide("Beta")],
            (9_144_000, 6_858_000),
            false,
        ),
        "placeholder" => (vec![placeholder_slide()], (9_144_000, 6_858_000), false),
        "table_chart" => (vec![table_chart_slide()], (9_144_000, 6_858_000), false),
        "notes_placeholder" => (vec![empty_slide()], (9_144_000, 6_858_000), true),
        _ => panic!("unknown deck: {name}"),
    };
    let chart = matches!(name, "table_chart");

    push(
        &mut p,
        "[Content_Types].xml",
        content_types(slide_xml.len(), notes, chart),
    );
    push(&mut p, "_rels/.rels", root_rels());
    push(&mut p, "docProps/core.xml", core_xml());
    push(&mut p, "ppt/theme/theme1.xml", theme_xml());
    push(&mut p, "ppt/slideMasters/slideMaster1.xml", master_xml());
    push(
        &mut p,
        "ppt/slideMasters/_rels/slideMaster1.xml.rels",
        master_rels_xml(),
    );
    push(&mut p, "ppt/slideLayouts/slideLayout1.xml", layout_xml());
    push(
        &mut p,
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
        rels(&[("rId1", REL_MASTER, "../slideMasters/slideMaster1.xml")]),
    );
    push(
        &mut p,
        "ppt/presentation.xml",
        presentation_xml(slide_size, slide_xml.len(), notes),
    );
    push(
        &mut p,
        "ppt/_rels/presentation.xml.rels",
        presentation_rels(slide_xml.len(), notes),
    );

    for (i, xml) in slide_xml.iter().enumerate() {
        let n = i + 1;
        push(&mut p, &format!("ppt/slides/slide{n}.xml"), xml);
        let mut slide_rels: Vec<(&str, &str, &str)> =
            vec![("rId1", REL_LAYOUT, "../slideLayouts/slideLayout1.xml")];
        if chart {
            slide_rels.push(("rId2", REL_CHART, "../charts/chart1.xml"));
        }
        if notes {
            slide_rels.push(("rId2", REL_NOTES_SLIDE, "../notesSlides/notesSlide1.xml"));
        }
        push(
            &mut p,
            &format!("ppt/slides/_rels/slide{n}.xml.rels"),
            rels(&slide_rels),
        );
    }

    if chart {
        push(&mut p, "ppt/charts/chart1.xml", chart_xml());
    }
    if notes {
        push(
            &mut p,
            "ppt/notesMasters/notesMaster1.xml",
            notes_master_xml(),
        );
        push(&mut p, "ppt/notesSlides/notesSlide1.xml", notes_slide_xml());
        push(
            &mut p,
            "ppt/notesSlides/_rels/notesSlide1.xml.rels",
            rels(&[("rId1", REL_NOTES_MASTER, "../notesMasters/notesMaster1.xml")]),
        );
    }
    p
}

fn push(p: &mut Vec<(String, Vec<u8>)>, uri: &str, xml: impl AsRef<[u8]>) {
    p.push((uri.to_string(), xml.as_ref().to_vec()));
}

fn rels(rels: &[(&str, &str, &str)]) -> String {
    let inner: String = rels
        .iter()
        .map(|(id, ty, target)| {
            format!("<Relationship Id=\"{id}\" Type=\"{ty}\" Target=\"{target}\"/>")
        })
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="{RELS}">{inner}</Relationships>"#
    )
}

fn content_types(slides: usize, notes: bool, chart: bool) -> String {
    let mut overrides = String::new();
    for n in 1..=slides {
        overrides.push_str(&format!(
            "<Override PartName=\"/ppt/slides/slide{n}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>"
        ));
    }
    if notes {
        overrides.push_str(
            "<Override PartName=\"/ppt/notesSlides/notesSlide1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml\"/><Override PartName=\"/ppt/notesMasters/notesMaster1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.notesMaster+xml\"/>",
        );
    }
    if chart {
        overrides.push_str(
            "<Override PartName=\"/ppt/charts/chart1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.drawingml.chart+xml\"/>",
        );
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="{CT}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/><Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/><Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>{overrides}</Types>"#
    )
}

fn root_rels() -> String {
    rels(&[("rId1", REL_OFFICE_DOC, "ppt/presentation.xml")])
}

fn presentation_xml(size: (i64, i64), slides: usize, notes: bool) -> String {
    let mut sld_ids = String::new();
    for i in 0..slides {
        sld_ids.push_str(&format!(
            "<p:sldId id=\"{}\" r:id=\"rId{}\"/>",
            256 + i as i64,
            3 + i as i64
        ));
    }
    let notes_master = if notes {
        "<p:notesMasterIdLst><p:notesMasterId r:id=\"rId2\"/></p:notesMasterIdLst>"
    } else {
        ""
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:presentation xmlns:a="{A}" xmlns:r="{R}" xmlns:p="{P}"><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst>{notes_master}<p:sldIdLst>{sld_ids}</p:sldIdLst><p:sldSz cx="{}" cy="{}"/><p:notesSz cx="6858000" cy="9144000"/><p:defaultTextStyle><a:defPPr><a:defRPr lang="en-US"/></a:defPPr></p:defaultTextStyle></p:presentation>"#,
        size.0, size.1
    )
}

fn presentation_rels(slides: usize, notes: bool) -> String {
    let mut rels = vec![
        (
            "rId1".to_string(),
            REL_MASTER,
            "slideMasters/slideMaster1.xml".to_string(),
        ),
        (
            "rId5".to_string(),
            REL_THEME,
            "theme/theme1.xml".to_string(),
        ),
    ];
    if notes {
        rels.push((
            "rId2".to_string(),
            REL_NOTES_MASTER,
            "notesMasters/notesMaster1.xml".to_string(),
        ));
    }
    for i in 0..slides {
        rels.push((
            format!("rId{}", 3 + i),
            REL_SLIDE,
            format!("slides/slide{}.xml", i + 1),
        ));
    }
    let inner: String = rels
        .iter()
        .map(|(id, ty, target)| {
            format!("<Relationship Id=\"{id}\" Type=\"{ty}\" Target=\"{target}\"/>")
        })
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="{RELS}">{inner}</Relationships>"#
    )
}

fn slide(xmlns_extra: &str, body: &str, closing: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sld xmlns:a="{A}" xmlns:p="{P}" xmlns:r="{R}"{xmlns_extra}><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>{body}</p:spTree></p:cSld>{closing}</p:sld>"#
    )
}

fn empty_slide() -> String {
    slide("", "", "<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>")
}

fn textbox_slide(text: &str) -> String {
    let body = format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="2" name="TextBox 1"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="914400" y="914400"/><a:ext cx="3657600" cy="914400"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/></p:spPr><p:txBody><a:bodyPr wrap="none"><a:spAutoFit/></a:bodyPr><a:lstStyle/><a:p><a:r><a:t>{text}</a:t></a:r></a:p></p:txBody></p:sp>"#
    );
    slide(
        "",
        &body,
        "<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>",
    )
}

fn placeholder_slide() -> String {
    let body = r#"<p:sp><p:nvSpPr><p:cNvPr id="2" name="Title 1"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="ctrTitle"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang="en-US"/></a:p></p:txBody></p:sp>"#;
    slide("", body, "<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>")
}

fn table_chart_slide() -> String {
    let body = r#"<p:sp><p:nvSpPr><p:cNvPr id="2" name="TextBox 1"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="914400" y="914400"/><a:ext cx="3657600" cy="914400"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/></p:spPr><p:txBody><a:bodyPr wrap="none"><a:spAutoFit/></a:bodyPr><a:lstStyle/><a:p><a:r><a:t>Header</a:t></a:r></a:p></p:txBody></p:sp><p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="3" name="Table 2"/><p:cNvGraphicFramePr><a:graphicFrameLocks noGrp="1"/></p:cNvGraphicFramePr><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x="914400" y="1828800"/><a:ext cx="3657600" cy="1371600"/></p:xfrm><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/table"><a:tbl><a:tblPr firstRow="1" bandRow="1"><a:tableStyleId>{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}</a:tableStyleId></a:tblPr><a:tblGrid><a:gridCol w="1828800"/><a:gridCol w="1828800"/></a:tblGrid><a:tr h="685800"><a:tc><a:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>H1</a:t></a:r></a:p></a:txBody><a:tcPr/></a:tc><a:tc><a:txBody><a:bodyPr/><a:lstStyle/><a:p/></a:txBody><a:tcPr/></a:tc></a:tr><a:tr h="685800"><a:tc><a:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>A</a:t></a:r></a:p></a:txBody><a:tcPr/></a:tc><a:tc><a:txBody><a:bodyPr/><a:lstStyle/><a:p/></a:txBody><a:tcPr/></a:tc></a:tr></a:tbl></a:graphicData></a:graphic></p:graphicFrame><p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="4" name="Chart 3"/><p:cNvGraphicFramePr><a:graphicFrameLocks noGrp="1"/></p:cNvGraphicFramePr><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x="4572000" y="1828800"/><a:ext cx="3657600" cy="1828800"/></p:xfrm><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" r:id="rId2"/></a:graphicData></a:graphic></p:graphicFrame>"#;
    slide("", body, "<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>")
}

fn chart_xml() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><c:date1904 val="0"/><c:chart><c:autoTitleDeleted val="0"/><c:plotArea><c:barChart><c:barDir val="col"/><c:grouping val="clustered"/><c:ser><c:idx val="0"/><c:order val="0"/><c:tx><c:strRef><c:f>Sheet1!$B$1</c:f><c:strCache><c:ptCount val="1"/><c:pt idx="0"><c:v>S1</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:f>Sheet1!$A$2:$A$3</c:f><c:strCache><c:ptCount val="2"/><c:pt idx="0"><c:v>Q1</c:v></c:pt><c:pt idx="1"><c:v>Q2</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:f>Sheet1!$B$2:$B$3</c:f><c:numCache><c:formatCode>General</c:formatCode><c:ptCount val="2"/><c:pt idx="0"><c:v>10</c:v></c:pt><c:pt idx="1"><c:v>20</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="-2068027336"/><c:axId val="-2113994440"/></c:barChart></c:plotArea></c:chart></c:chartSpace>"#.to_string()
}

/// The slide layout every slide references. It owns a `ctrTitle` placeholder
/// whose geometry and text defaults are inherited by slides that use one.
fn layout_xml() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sldLayout xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" type="title" preserve="1"><p:cSld name="Title Slide"><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/><p:sp><p:nvSpPr><p:cNvPr id="2" name="Title 1"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="ctrTitle"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="685800" y="2130425"/><a:ext cx="7772400" cy="1470025"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:solidFill><a:srgbClr val="C7000B"/></a:solidFill><a:ln><a:noFill/></a:ln></p:spPr><p:txBody><a:bodyPr anchor="t"><a:normAutofit/></a:bodyPr><a:lstStyle><a:lvl1pPr algn="ctr"><a:defRPr sz="3200" b="1"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill><a:latin typeface="Calibri"/></a:defRPr></a:lvl1pPr></a:lstStyle><a:p><a:endParaRPr lang="en-US"/></a:p></p:txBody></p:sp></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"#.to_string()
}

fn master_xml() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sldMaster xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld name="Office Theme"><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/><p:sp><p:nvSpPr><p:cNvPr id="2" name="Title Placeholder 1"/><p:cNvSpPr/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="457200" y="274638"/><a:ext cx="8229600" cy="1143000"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang="en-US"/></a:p></p:txBody></p:sp><p:sp><p:nvSpPr><p:cNvPr id="3" name="Text Placeholder 2"/><p:cNvSpPr/><p:nvPr><p:ph type="body"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="457200" y="1470025"/><a:ext cx="8229600" cy="4114800"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang="en-US"/></a:p></p:txBody></p:sp></p:spTree></p:cSld><p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/><p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst><p:txStyles><p:titleStyle/><p:bodyStyle/><p:otherStyle/></p:txStyles></p:sldMaster>"#.to_string()
}

/// A master rels file that also owns the shared theme, so the mirror's
/// slide-master section and the theme round-trip tests behave like a real deck.
fn master_rels_xml() -> String {
    rels(&[
        ("rId1", REL_LAYOUT, "../slideLayouts/slideLayout1.xml"),
        ("rId2", REL_THEME, "../theme/theme1.xml"),
    ])
}

/// The python-pptx "Office" palette the CLI tests assert on.
fn theme_xml() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Office Theme"><a:themeElements><a:clrScheme name="Office"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="1F497D"/></a:dk2><a:lt2><a:srgbClr val="EEECE1"/></a:lt2><a:accent1><a:srgbClr val="4F81BD"/></a:accent1><a:accent2><a:srgbClr val="C0504D"/></a:accent2><a:accent3><a:srgbClr val="9BBB59"/></a:accent3><a:accent4><a:srgbClr val="8064A2"/></a:accent4><a:accent5><a:srgbClr val="4BACC6"/></a:accent5><a:accent6><a:srgbClr val="F79646"/></a:accent6><a:hlink><a:srgbClr val="0000FF"/></a:hlink><a:folHlink><a:srgbClr val="800080"/></a:folHlink></a:clrScheme><a:fontScheme name="Office"><a:majorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme><a:fmtScheme name="Office"><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:fillStyleLst><a:lnStyleLst><a:ln w="6350" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/></a:ln><a:ln w="12700" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/></a:ln><a:ln w="19050" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/></a:ln></a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>"#.to_string()
}

fn core_xml() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:dcmitype="http://purl.org/dc/dcmitype/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><dc:title/><dc:subject/><dc:creator/><cp:keywords/><dc:description>generated using python-pptx</dc:description><cp:lastModifiedBy>Steve Canny</cp:lastModifiedBy><cp:revision>1</cp:revision><dcterms:created xsi:type="dcterms:W3CDTF">2013-01-27T09:14:16Z</dcterms:created><dcterms:modified xsi:type="dcterms:W3CDTF">2013-01-27T09:15:58Z</dcterms:modified><cp:category/></cp:coreProperties>"#.to_string()
}

/// notesMaster with an `sldImg` placeholder that owns a `p:txBody` — the
/// round-trip regression counterpart to `notes_slide_xml`.
fn notes_master_xml() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:notesMaster xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/><p:sp><p:nvSpPr><p:cNvPr id="2" name="Slide Image Placeholder 3"/><p:cNvSpPr><a:spLocks noGrp="1" noRot="1" noChangeAspect="1"/></p:cNvSpPr><p:nvPr><p:ph type="sldImg" idx="2"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="685800" y="1143000"/><a:ext cx="5486400" cy="3086100"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/><a:ln w="12700"><a:solidFill><a:prstClr val="black"/></a:solidFill></a:ln></p:spPr><p:txBody><a:bodyPr vert="horz" lIns="91440" tIns="45720" rIns="91440" bIns="45720" rtlCol="0" anchor="ctr"/><a:lstStyle/><a:p><a:endParaRPr lang="en-US"/></a:p></p:txBody></p:sp><p:sp><p:nvSpPr><p:cNvPr id="3" name="Notes Placeholder 4"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="body" idx="3"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="685800" y="4400550"/><a:ext cx="5486400" cy="3600450"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr><p:txBody><a:bodyPr vert="horz" lIns="91440" tIns="45720" rIns="91440" bIns="45720" rtlCol="0"/><a:lstStyle/><a:p><a:pPr lvl="0"/><a:r><a:rPr lang="en-US"/><a:t>Edit Master text styles</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld><p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/><p:notesStyle><a:lvl1pPr marL="0" algn="l" defTabSz="457200" rtl="0" eaLnBrk="1" latinLnBrk="0" hangingPunct="1"><a:defRPr sz="1800" kern="1200"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill><a:latin typeface="+mn-lt"/><a:ea typeface="+mn-ea"/><a:cs typeface="+mn-cs"/></a:defRPr></a:lvl1pPr></p:notesStyle></p:notesMaster>"#.to_string()
}

/// notesSlide whose `sldImg` placeholder has NO `p:txBody`, reproducing the
/// round-trip bug: placeholder resolution must not fabricate a text frame the
/// shape does not own.
fn notes_slide_xml() -> String {
    let body = r#"<p:sp><p:nvSpPr><p:cNvPr id="2" name="Slide Image Placeholder 1"/><p:cNvSpPr><a:spLocks noGrp="1" noRot="1" noChangeAspect="1"/></p:cNvSpPr><p:nvPr><p:ph type="sldImg"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="685800" y="1143000"/><a:ext cx="5486400" cy="3086100"/></a:xfrm></p:spPr></p:sp><p:sp><p:nvSpPr><p:cNvPr id="3" name="Notes Placeholder 2"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang="en-US" dirty="0"/></a:p></p:txBody></p:sp>"#;
    slide("", body, "")
}

#[cfg(test)]
mod tests {
    use super::*;
    use gwen_pptx::opc::Package;

    #[test]
    fn every_deck_opens_and_is_structurally_sound() {
        for name in DECKS {
            let path = deck(name);
            let pkg = Package::open(&path).expect("deck opens");

            for (source, rels) in pkg.rels_uris() {
                for rel in rels.values() {
                    if rel.target_mode.as_deref() == Some("External") {
                        continue;
                    }
                    if let Some(target) = pkg.resolve_relationship_target(source, rel) {
                        assert!(
                            pkg.part_exists(&target),
                            "{name}: {source} -> {target} missing"
                        );
                    }
                }
            }

            let ct = String::from_utf8(
                pkg.get_part("[Content_Types].xml")
                    .expect("content types")
                    .to_vec(),
            )
            .unwrap();
            for uri in pkg.part_uris() {
                let ext = uri.rsplit('.').next().unwrap_or("");
                assert!(
                    ct.contains(&format!("Default Extension=\"{ext}\""))
                        || ct.contains(&format!("PartName=\"/{uri}\"")),
                    "{name}: {uri} has no content type"
                );
            }
        }
    }
}
