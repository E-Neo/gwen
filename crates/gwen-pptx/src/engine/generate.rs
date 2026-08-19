use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use std::io::{Cursor, Write};

use serde_json::{Map, Value};

use crate::dto::{ChartDto, ShapeDto};

fn writer() -> Writer<Cursor<Vec<u8>>> {
    Writer::new(Cursor::new(Vec::new()))
}

fn start(w: &mut Writer<Cursor<Vec<u8>>>, name: &str, attrs: &[(&str, &str)]) {
    let mut e = BytesStart::new(name);
    for (k, v) in attrs {
        e.push_attribute((*k, *v));
    }
    w.write_event(Event::Start(e)).unwrap();
}

fn end(w: &mut Writer<Cursor<Vec<u8>>>, name: &str) {
    w.write_event(Event::End(BytesEnd::new(name))).unwrap();
}

fn empty(w: &mut Writer<Cursor<Vec<u8>>>, name: &str, attrs: &[(&str, &str)]) {
    let mut e = BytesStart::new(name);
    for (k, v) in attrs {
        e.push_attribute((*k, *v));
    }
    w.write_event(Event::Empty(e)).unwrap();
}

fn text(w: &mut Writer<Cursor<Vec<u8>>>, name: &str, val: &str) {
    start(w, name, &[]);
    w.write_event(Event::Text(BytesText::new(val))).unwrap();
    end(w, name);
}

/// The `p:spTree` body for a set of shapes: the group non-visual properties
/// plus each shape's XML. The surrounding `p:spTree` open/close tags are owned
/// by the calling splice logic.
pub fn sp_tree_body(shapes: &[ShapeDto]) -> String {
    let mut w = writer();
    start(&mut w, "p:nvGrpSpPr", &[]);
    empty(&mut w, "p:cNvPr", &[("id", "1"), ("name", "")]);
    empty(&mut w, "p:cNvGrpSpPr", &[]);
    empty(&mut w, "p:nvPr", &[]);
    end(&mut w, "p:nvGrpSpPr");

    start(&mut w, "p:grpSpPr", &[]);
    start(&mut w, "a:xfrm", &[]);
    empty(&mut w, "a:off", &[("x", "0"), ("y", "0")]);
    empty(&mut w, "a:ext", &[("cx", "0"), ("cy", "0")]);
    empty(&mut w, "a:chOff", &[("x", "0"), ("y", "0")]);
    empty(&mut w, "a:chExt", &[("cx", "0"), ("cy", "0")]);
    end(&mut w, "a:xfrm");
    end(&mut w, "p:grpSpPr");

    for shape in shapes {
        let xml = crate::dto::xml::shape_to_xml(shape);
        w.get_mut().write_all(xml.as_bytes()).unwrap();
    }
    String::from_utf8(w.into_inner().into_inner()).expect("valid UTF-8")
}

/// A slide/master/layout/notes reference to write into `p:sldIdLst` etc.
pub struct ListEntry {
    pub id: u32,
    pub r_id: String,
}

/// Generate a `p:presentation` part from the slide/master wiring and geometry.
/// `tail` carries the unmodeled trailing children (`p:defaultTextStyle`,
/// `p:extLst`) verbatim.
pub fn presentation_xml(
    masters: &[ListEntry],
    notes_master_r_id: Option<&str>,
    slides: &[ListEntry],
    width: i64,
    height: i64,
    tail: &[u8],
) -> Vec<u8> {
    let mut w = writer();
    w.write_event(Event::Decl(BytesDecl::new(
        "1.0",
        Some("UTF-8"),
        Some("yes"),
    )))
    .unwrap();
    let mut root = BytesStart::new("p:presentation");
    root.push_attribute((
        "xmlns:a",
        "http://schemas.openxmlformats.org/drawingml/2006/main",
    ));
    root.push_attribute((
        "xmlns:r",
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
    ));
    root.push_attribute((
        "xmlns:p",
        "http://schemas.openxmlformats.org/presentationml/2006/main",
    ));
    w.write_event(Event::Start(root)).unwrap();

    start(&mut w, "p:sldMasterIdLst", &[]);
    for m in masters {
        empty(
            &mut w,
            "p:sldMasterId",
            &[("id", &m.id.to_string()), ("r:id", &m.r_id)],
        );
    }
    end(&mut w, "p:sldMasterIdLst");

    if let Some(rid) = notes_master_r_id {
        start(&mut w, "p:notesMasterIdLst", &[]);
        empty(&mut w, "p:notesMasterId", &[("r:id", rid)]);
        end(&mut w, "p:notesMasterIdLst");
    }

    start(&mut w, "p:sldIdLst", &[]);
    for s in slides {
        empty(
            &mut w,
            "p:sldId",
            &[("id", &s.id.to_string()), ("r:id", &s.r_id)],
        );
    }
    end(&mut w, "p:sldIdLst");

    empty(
        &mut w,
        "p:sldSz",
        &[("cx", &width.to_string()), ("cy", &height.to_string())],
    );
    empty(&mut w, "p:notesSz", &[("cx", "6858000"), ("cy", "9144000")]);

    w.get_mut().write_all(tail).unwrap();
    end(&mut w, "p:presentation");
    w.into_inner().into_inner()
}

/// The ordered theme color names used by `a:clrScheme`.
pub const THEME_COLOR_NAMES: [&str; 12] = [
    "dk1", "lt1", "dk2", "lt2", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6",
    "hlink", "folHlink",
];

/// Generate a `a:theme` part. `tail` carries the unmodeled trailing children
/// (`a:fmtScheme`, `a:objectDefaults`, ...) verbatim.
pub fn theme_xml(colors: &Map<String, Value>, fonts: &Map<String, Value>, tail: &[u8]) -> Vec<u8> {
    let mut w = writer();
    w.write_event(Event::Decl(BytesDecl::new(
        "1.0",
        Some("UTF-8"),
        Some("yes"),
    )))
    .unwrap();
    let mut root = BytesStart::new("a:theme");
    root.push_attribute(("name", "Office Theme"));
    root.push_attribute((
        "xmlns:a",
        "http://schemas.openxmlformats.org/drawingml/2006/main",
    ));
    w.write_event(Event::Start(root)).unwrap();

    start(&mut w, "a:themeElements", &[]);

    start(&mut w, "a:clrScheme", &[("name", "Office")]);
    for name in THEME_COLOR_NAMES {
        let val = colors.get(name).and_then(Value::as_str).unwrap_or("");
        start(&mut w, &format!("a:{name}"), &[]);
        empty(&mut w, "a:srgbClr", &[("val", val)]);
        end(&mut w, &format!("a:{name}"));
    }
    end(&mut w, "a:clrScheme");

    start(&mut w, "a:fontScheme", &[("name", "Office")]);
    for (key, family) in [("majorFont", "major"), ("minorFont", "minor")] {
        start(&mut w, &format!("a:{key}"), &[]);
        let typeface = fonts
            .get(family)
            .and_then(Value::as_str)
            .unwrap_or("Calibri");
        empty(&mut w, "a:latin", &[("typeface", typeface)]);
        end(&mut w, &format!("a:{key}"));
    }
    end(&mut w, "a:fontScheme");

    w.get_mut().write_all(tail).unwrap();
    end(&mut w, "a:themeElements");
    end(&mut w, "a:theme");
    w.into_inner().into_inner()
}

/// Generate the `p:bg` element for a solid-fill slide background. Returns `None`
/// when the background carries no editable fill (kept from the fragment).
pub fn slide_background_xml(bg: &Value) -> Option<Vec<u8>> {
    let fill = bg
        .as_object()
        .and_then(|o| o.get("fill"))
        .and_then(Value::as_object)?;
    let ty = fill.get("type").and_then(Value::as_str).unwrap_or("");
    if ty.is_empty() || ty == "SOLID" {
        // Background colors are plain hex strings in the mirror.
        let color = fill.get("color").and_then(Value::as_str).unwrap_or("");
        let mut w = writer();
        start(&mut w, "p:bg", &[]);
        start(&mut w, "p:bgPr", &[]);
        start(&mut w, "a:solidFill", &[]);
        if color.is_empty() {
            empty(&mut w, "a:srgbClr", &[("val", "4472C4")]);
        } else {
            empty(&mut w, "a:srgbClr", &[("val", color)]);
        }
        end(&mut w, "a:solidFill");
        empty(&mut w, "a:effectLst", &[]);
        end(&mut w, "p:bgPr");
        end(&mut w, "p:bg");
        return Some(w.into_inner().into_inner());
    }
    None
}

/// Generate a chart part (`c:chartSpace`) with literal series caches from a
/// readable chart definition.
pub fn chart_xml(chart: &ChartDto) -> Vec<u8> {
    let mut w = writer();
    w.write_event(Event::Decl(BytesDecl::new(
        "1.0",
        Some("UTF-8"),
        Some("yes"),
    )))
    .unwrap();
    let mut root = BytesStart::new("c:chartSpace");
    root.push_attribute((
        "xmlns:c",
        "http://schemas.openxmlformats.org/drawingml/2006/chart",
    ));
    root.push_attribute((
        "xmlns:a",
        "http://schemas.openxmlformats.org/drawingml/2006/main",
    ));
    root.push_attribute((
        "xmlns:r",
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
    ));
    w.write_event(Event::Start(root)).unwrap();

    start(&mut w, "c:chart", &[]);
    start(&mut w, "c:plotArea", &[]);
    empty(&mut w, "c:layout", &[]);

    let chart_tag = chart
        .chart_type
        .as_deref()
        .filter(|t| t.starts_with("c:"))
        .map(str::to_string)
        .unwrap_or_else(|| "c:barChart".to_string());
    start(&mut w, &chart_tag, &[]);
    for (i, series) in chart.series.iter().enumerate() {
        let xml = crate::dto::xml::chart_series_to_xml(series, i);
        w.get_mut().write_all(xml.as_bytes()).unwrap();
    }
    end(&mut w, &chart_tag);
    end(&mut w, "c:plotArea");
    end(&mut w, "c:chart");
    end(&mut w, "c:chartSpace");
    w.into_inner().into_inner()
}

/// Generate `docProps/core.xml` from the mirrored core properties object.
pub fn core_props_xml(props: &Value) -> Vec<u8> {
    let mut w = writer();
    w.write_event(Event::Decl(BytesDecl::new(
        "1.0",
        Some("UTF-8"),
        Some("yes"),
    )))
    .unwrap();
    let mut root = BytesStart::new("cp:coreProperties");
    root.push_attribute((
        "xmlns:cp",
        "http://schemas.openxmlformats.org/package/2006/metadata/core-properties",
    ));
    root.push_attribute(("xmlns:dc", "http://purl.org/dc/elements/1.1/"));
    root.push_attribute(("xmlns:dcterms", "http://purl.org/dc/terms/"));
    root.push_attribute(("xmlns:dcmitype", "http://purl.org/dc/dcmitype/"));
    root.push_attribute(("xmlns:xsi", "http://www.w3.org/2001/XMLSchema-instance"));
    w.write_event(Event::Start(root)).unwrap();

    let obj = props.as_object();
    let get = |k: &str| {
        obj.and_then(|o| o.get(k))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
    };
    for (key, tag) in [
        ("title", "dc:title"),
        ("subject", "dc:subject"),
        ("author", "dc:creator"),
        ("keywords", "cp:keywords"),
        ("comments", "dc:description"),
        ("last_modified_by", "cp:lastModifiedBy"),
        ("revision", "cp:revision"),
        ("category", "cp:category"),
        ("content_status", "cp:contentStatus"),
    ] {
        if let Some(v) = get(key) {
            text(&mut w, tag, v);
        }
    }
    if let Some(v) = get("created") {
        start(&mut w, "dcterms:created", &[("xsi:type", "dcterms:W3CDTF")]);
        w.write_event(Event::Text(BytesText::new(v))).unwrap();
        end(&mut w, "dcterms:created");
    }
    if let Some(v) = get("modified") {
        start(
            &mut w,
            "dcterms:modified",
            &[("xsi:type", "dcterms:W3CDTF")],
        );
        w.write_event(Event::Text(BytesText::new(v))).unwrap();
        end(&mut w, "dcterms:modified");
    }
    end(&mut w, "cp:coreProperties");
    w.into_inner().into_inner()
}

/// A part entry for content-type generation.
pub struct PartEntry {
    pub uri: String,
    pub content_type: Option<String>,
}

/// Generate `[Content_Types].xml` from the final part list. `overrides` carries
/// the recorded content-type overrides for preserved parts; defaults are
/// derived from file extensions.
pub fn content_types_xml(parts: &[PartEntry], defaults: &[(&str, &str)]) -> Vec<u8> {
    let mut w = writer();
    w.write_event(Event::Decl(BytesDecl::new(
        "1.0",
        Some("UTF-8"),
        Some("yes"),
    )))
    .unwrap();
    let mut root = BytesStart::new("Types");
    root.push_attribute((
        "xmlns",
        "http://schemas.openxmlformats.org/package/2006/content-types",
    ));
    w.write_event(Event::Start(root)).unwrap();

    let mut seen = std::collections::HashSet::new();
    for (ext, ct) in defaults {
        if seen.insert(ext) {
            empty(
                &mut w,
                "Default",
                &[("Extension", ext), ("ContentType", ct)],
            );
        }
    }
    empty(
        &mut w,
        "Default",
        &[
            ("Extension", "rels"),
            (
                "ContentType",
                "application/vnd.openxmlformats-package.relationships+xml",
            ),
        ],
    );
    empty(
        &mut w,
        "Default",
        &[("Extension", "xml"), ("ContentType", "application/xml")],
    );

    for p in parts {
        let Some(ct) = &p.content_type else {
            continue;
        };
        let mut e = BytesStart::new("Override");
        e.push_attribute(("PartName", format!("/{}", p.uri).as_str()));
        e.push_attribute(("ContentType", ct.as_str()));
        w.write_event(Event::Empty(e)).unwrap();
    }
    end(&mut w, "Types");
    w.into_inner().into_inner()
}
