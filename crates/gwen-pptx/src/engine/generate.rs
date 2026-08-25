use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use std::io::{Cursor, Write};

use serde_json::{Map, Value};

use crate::dto::{ChartDto, ShapeDto};
use crate::error::AppResult;

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
/// Unmodeled structural children (`p:defaultTextStyle`, `p:extLst`) are
/// regenerated from standard Office defaults.
pub fn presentation_xml(
    masters: &[ListEntry],
    notes_master_r_id: Option<&str>,
    slides: &[ListEntry],
    width: i64,
    height: i64,
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

    w.get_mut()
        .write_all(default_text_style_xml().as_bytes())
        .unwrap();
    end(&mut w, "p:presentation");
    w.into_inner().into_inner()
}

/// Standard Office default text style (`p:defaultTextStyle`) with no explicit
/// paragraph properties.
fn default_text_style_xml() -> String {
    let mut w = writer();
    start(&mut w, "p:defaultTextStyle", &[]);
    start(&mut w, "a:defPPr", &[]);
    start(&mut w, "a:defRPr", &[("lang", "en-US")]);
    start(&mut w, "a:solidFill", &[]);
    empty(&mut w, "a:schemeClr", &[("val", "tx1")]);
    end(&mut w, "a:solidFill");
    empty(&mut w, "a:latin", &[("typeface", "+mj-lt")]);
    empty(&mut w, "a:ea", &[("typeface", "+mj-ea")]);
    empty(&mut w, "a:cs", &[("typeface", "+mj-cs")]);
    end(&mut w, "a:defRPr");
    end(&mut w, "a:defPPr");
    end(&mut w, "p:defaultTextStyle");
    String::from_utf8(w.into_inner().into_inner()).expect("valid UTF-8")
}

/// The ordered theme color names used by `a:clrScheme`.
pub const THEME_COLOR_NAMES: [&str; 12] = [
    "dk1", "lt1", "dk2", "lt2", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6",
    "hlink", "folHlink",
];

/// Office default palette value for a `clrScheme` slot the mirror omits or
/// writes in a form that is not a 6-digit hex color.
fn default_theme_color(slot: &str) -> &'static str {
    match slot {
        "dk2" => "1F497D",
        "lt2" => "EEECE1",
        "accent1" => "4F81BD",
        "accent2" => "C0504D",
        "accent3" => "9BBB59",
        "accent4" => "8064A2",
        "accent5" => "4BACC6",
        "accent6" => "F79646",
        "hlink" => "0000FF",
        _ => "800080",
    }
}

/// True when `s` satisfies the `ST_HexColorRGB` grammar (6 hex digits).
fn is_hex_color(s: &str) -> bool {
    s.len() == 6 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// One `a:<slot>` element of `a:clrScheme`. System-background slots (`dk1`,
/// `lt1`) fall back to their `a:sysClr` form when the mirror carries no hex
/// value; every other invalid or missing slot falls back to the Office
/// palette, because `a:srgbClr/@val` must be exactly 6 hex digits.
fn write_color_slot(w: &mut Writer<Cursor<Vec<u8>>>, slot: &str, val: &str) {
    start(w, &format!("a:{slot}"), &[]);
    let tag = format!("a:{slot}");
    if (slot == "dk1" || slot == "lt1") && !is_hex_color(val) {
        let (sys, last) = if slot == "dk1" {
            ("windowText", "000000")
        } else {
            ("window", "FFFFFF")
        };
        empty(w, "a:sysClr", &[("val", sys), ("lastClr", last)]);
    } else if is_hex_color(val) {
        empty(w, "a:srgbClr", &[("val", val)]);
    } else {
        empty(w, "a:srgbClr", &[("val", default_theme_color(slot))]);
    }
    end(w, &tag);
}

/// Generate a `a:theme` part. The `fmtScheme` and `objectDefaults` children are
/// regenerated from standard Office defaults.
pub fn theme_xml(colors: &Map<String, Value>, fonts: &Map<String, Value>) -> Vec<u8> {
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
        write_color_slot(&mut w, name, val);
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
        // `CT_FontCollection` requires the ea and cs faces alongside latin.
        empty(&mut w, "a:ea", &[("typeface", "")]);
        empty(&mut w, "a:cs", &[("typeface", "")]);
        end(&mut w, &format!("a:{key}"));
    }
    end(&mut w, "a:fontScheme");

    w.get_mut().write_all(fmt_scheme_xml().as_bytes()).unwrap();
    end(&mut w, "a:themeElements");
    end(&mut w, "a:theme");
    w.into_inner().into_inner()
}

/// Standard Office format scheme (`a:fmtScheme`). `CT_FillStyleList`,
/// `CT_LineStyleList` and `CT_EffectStyleList` each require exactly three
/// entries; effect colors nest their transforms inside the color element.
fn fmt_scheme_xml() -> String {
    let mut w = writer();
    start(&mut w, "a:fmtScheme", &[("name", "Office")]);

    start(&mut w, "a:fillStyleLst", &[]);
    start(&mut w, "a:solidFill", &[]);
    empty(&mut w, "a:schemeClr", &[("val", "phClr")]);
    end(&mut w, "a:solidFill");
    write_gradient_fill(&mut w, "30000", "70000");
    write_gradient_fill(&mut w, "100000", "0");
    end(&mut w, "a:fillStyleLst");

    start(&mut w, "a:lnStyleLst", &[]);
    for wd in ["6350", "12700", "19050"] {
        start(
            &mut w,
            "a:ln",
            &[("w", wd), ("cap", "flat"), ("cmpd", "sng"), ("algn", "ctr")],
        );
        start(&mut w, "a:solidFill", &[]);
        empty(&mut w, "a:schemeClr", &[("val", "phClr")]);
        end(&mut w, "a:solidFill");
        start(&mut w, "a:prstDash", &[]);
        empty(&mut w, "a:prstDash", &[("val", "solid")]);
        end(&mut w, "a:prstDash");
        end(&mut w, "a:ln");
    }
    end(&mut w, "a:lnStyleLst");

    start(&mut w, "a:effectStyleLst", &[]);
    for blur in ["0", "50800"] {
        start(&mut w, "a:effectStyle", &[]);
        if blur == "0" {
            empty(&mut w, "a:effectLst", &[]);
        } else {
            start(&mut w, "a:effectLst", &[]);
            write_shadow_color(&mut w, "63500", "38100");
            end(&mut w, "a:effectLst");
        }
        end(&mut w, "a:effectStyle");
    }
    // The third entry carries the stronger shadow Office applies to
    // placeholders.
    start(&mut w, "a:effectStyle", &[]);
    start(&mut w, "a:effectLst", &[]);
    write_shadow_color(&mut w, "50800", "38100");
    end(&mut w, "a:effectLst");
    end(&mut w, "a:effectStyle");
    end(&mut w, "a:effectStyleLst");

    start(&mut w, "a:bgFillStyleLst", &[]);
    start(&mut w, "a:solidFill", &[]);
    empty(&mut w, "a:schemeClr", &[("val", "phClr")]);
    end(&mut w, "a:solidFill");
    write_gradient_fill(&mut w, "102000", "0");
    write_gradient_fill(&mut w, "100000", "0");
    end(&mut w, "a:bgFillStyleLst");

    end(&mut w, "a:fmtScheme");
    String::from_utf8(w.into_inner().into_inner()).expect("valid UTF-8")
}

/// A two-stop vertical `a:gradFill` over `phClr` with a lumMod/lumOff pair on
/// the first stop.
fn write_gradient_fill(w: &mut Writer<Cursor<Vec<u8>>>, lum_mod: &str, lum_off: &str) {
    start(w, "a:gradFill", &[("rotWithShape", "1")]);
    start(w, "a:gsLst", &[]);
    start(w, "a:gs", &[("pos", "0")]);
    start(w, "a:schemeClr", &[("val", "phClr")]);
    empty(w, "a:lumMod", &[("val", lum_mod)]);
    empty(w, "a:lumOff", &[("val", lum_off)]);
    end(w, "a:schemeClr");
    end(w, "a:gs");
    start(w, "a:gs", &[("pos", "100000")]);
    empty(w, "a:schemeClr", &[("val", "phClr")]);
    end(w, "a:gs");
    end(w, "a:gsLst");
    empty(w, "a:lin", &[("ang", "5400000"), ("scaled", "0")]);
    end(w, "a:gradFill");
}

/// An `a:outerShdw` whose black color carries its `a:alpha` transform as a
/// child element (the schema has no sibling-transform form).
fn write_shadow_color(w: &mut Writer<Cursor<Vec<u8>>>, blur_rad: &str, dist: &str) {
    start(
        w,
        "a:outerShdw",
        &[
            ("blurRad", blur_rad),
            ("dist", dist),
            ("dir", "5400000"),
            ("rotWithShape", "0"),
        ],
    );
    start(w, "a:srgbClr", &[("val", "000000")]);
    empty(w, "a:alpha", &[("val", "40000")]);
    end(w, "a:srgbClr");
    end(w, "a:outerShdw");
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
/// readable chart definition. Bar charts carry the category/value axis pair
/// the schema requires (`c:axId` refs plus `c:catAx`/`c:valAx`); pie charts
/// have no axes. Anything else is rejected — an incomplete chart part would
/// make the whole package invalid.
pub fn chart_xml(chart: &ChartDto) -> AppResult<Vec<u8>> {
    use crate::error::AppError;
    let kind = chart.chart_type.as_deref().unwrap_or("c:barChart");
    if kind != "c:barChart" && kind != "c:pieChart" {
        return Err(AppError::InvalidValue(format!(
            "unsupported chart type `{kind}` (supported: c:barChart, c:pieChart)"
        )));
    }
    let pie = kind == "c:pieChart";
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
    empty(&mut w, "c:autoTitleDeleted", &[("val", "1")]);
    start(&mut w, "c:plotArea", &[]);
    empty(&mut w, "c:layout", &[]);
    if pie {
        write_pie_chart(&mut w, chart);
    } else {
        write_bar_chart(&mut w, chart);
        write_cat_ax(&mut w, CAT_AX_ID, VAL_AX_ID);
        write_val_ax(&mut w, VAL_AX_ID, CAT_AX_ID);
    }
    end(&mut w, "c:plotArea");
    empty(&mut w, "c:plotVisOnly", &[("val", "1")]);
    end(&mut w, "c:chart");
    end(&mut w, "c:chartSpace");
    Ok(w.into_inner().into_inner())
}

/// Fixed axis ids binding a generated bar chart's `c:axId` refs to its axes.
const CAT_AX_ID: &str = "111111111";
const VAL_AX_ID: &str = "222222222";

fn write_series(w: &mut Writer<Cursor<Vec<u8>>>, chart: &ChartDto) {
    for (i, series) in chart.series.iter().enumerate() {
        let xml = crate::dto::xml::chart_series_to_xml(series, i);
        w.get_mut().write_all(xml.as_bytes()).unwrap();
    }
}

fn write_bar_chart(w: &mut Writer<Cursor<Vec<u8>>>, chart: &ChartDto) {
    start(w, "c:barChart", &[]);
    empty(w, "c:barDir", &[("val", "col")]);
    empty(w, "c:grouping", &[("val", "clustered")]);
    empty(w, "c:varyColors", &[("val", "0")]);
    write_series(w, chart);
    empty(w, "c:gapWidth", &[("val", "150")]);
    empty(w, "c:axId", &[("val", CAT_AX_ID)]);
    empty(w, "c:axId", &[("val", VAL_AX_ID)]);
    end(w, "c:barChart");
}

fn write_pie_chart(w: &mut Writer<Cursor<Vec<u8>>>, chart: &ChartDto) {
    start(w, "c:pieChart", &[]);
    empty(w, "c:varyColors", &[("val", "1")]);
    write_series(w, chart);
    empty(w, "c:firstSliceAng", &[("val", "0")]);
    end(w, "c:pieChart");
}

/// A minimal but schema-complete category axis (`c:catAx`).
fn write_cat_ax(w: &mut Writer<Cursor<Vec<u8>>>, ax_id: &str, cross_id: &str) {
    start(w, "c:catAx", &[]);
    empty(w, "c:axId", &[("val", ax_id)]);
    write_scaling(w);
    empty(w, "c:delete", &[("val", "0")]);
    empty(w, "c:axPos", &[("val", "b")]);
    empty(w, "c:crossAx", &[("val", cross_id)]);
    end(w, "c:catAx");
}

/// A minimal but schema-complete value axis (`c:valAx`).
fn write_val_ax(w: &mut Writer<Cursor<Vec<u8>>>, ax_id: &str, cross_id: &str) {
    start(w, "c:valAx", &[]);
    empty(w, "c:axId", &[("val", ax_id)]);
    write_scaling(w);
    empty(w, "c:delete", &[("val", "0")]);
    empty(w, "c:axPos", &[("val", "l")]);
    empty(w, "c:crossAx", &[("val", cross_id)]);
    end(w, "c:valAx");
}

fn write_scaling(w: &mut Writer<Cursor<Vec<u8>>>) {
    start(w, "c:scaling", &[]);
    empty(w, "c:orientation", &[("val", "minMax")]);
    end(w, "c:scaling");
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
