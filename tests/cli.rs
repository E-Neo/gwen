use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::Value;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_pptx-engineer")
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn tmp() -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "pptx-engineer-it-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run_ok(args: &[&str]) -> String {
    let out = Command::new(bin()).args(args).output().expect("run binary");
    assert!(
        out.status.success(),
        "command failed: {args:?}\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn query(input: &Path, path: &str) -> Value {
    let out = run_ok(&["query", input.to_str().unwrap(), "--path", path]);
    serde_json::from_str(&out).unwrap()
}

fn read_zip_entry(path: &Path, name: &str) -> String {
    let file = std::fs::File::open(path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let mut entry = zip.by_name(name).unwrap();
    let mut buf = String::new();
    std::io::Read::read_to_string(&mut entry, &mut buf).unwrap();
    buf
}

#[test]
fn new_creates_wide_deck() {
    let dir = tmp();
    let deck = dir.join("deck.pptx");
    run_ok(&["new", deck.to_str().unwrap(), "--size", "16:9"]);

    let pres = query(&deck, "p");
    assert_eq!(pres["slide_width"], 12192000);
    assert_eq!(pres["slide_height"], 6858000);
    assert_eq!(pres["slides"].as_array().unwrap().len(), 1);
    assert_eq!(pres["slides"][0]["shapes"].as_array().unwrap().len(), 0);
    assert!(pres["slides"][0]["background"]["fill"]["type"].is_null());
}

#[test]
fn new_creates_standard_deck() {
    let dir = tmp();
    let deck = dir.join("deck.pptx");
    run_ok(&["new", deck.to_str().unwrap(), "--size", "4:3"]);
    let pres = query(&deck, "p");
    assert_eq!(pres["slide_width"], 9144000);
    assert_eq!(pres["slide_height"], 6858000);
}

#[test]
fn replace_core_properties_roundtrip() {
    let dir = tmp();
    let deck = dir.join("deck.pptx");
    run_ok(&["new", deck.to_str().unwrap()]);
    let out = deck.with_extension("out.pptx");
    run_ok(&[
        "replace",
        deck.to_str().unwrap(),
        "--path",
        "p.core_properties.title",
        "--value",
        r#""My Deck""#,
        "--output",
        out.to_str().unwrap(),
    ]);
    let pres = query(&out, "p");
    assert_eq!(pres["core_properties"]["title"], "My Deck");
}

#[test]
fn replace_text_roundtrip() {
    let dir = tmp();
    let out = dir.join("text.pptx");
    run_ok(&[
        "replace",
        fixture("two_slides.pptx").to_str().unwrap(),
        "--path",
        "slides[0].shapes[0].text_frame.paragraphs[0].runs[0].text",
        "--value",
        r#""Changed""#,
        "--output",
        out.to_str().unwrap(),
    ]);
    let v = query(
        &out,
        "slides[0].shapes[0].text_frame.paragraphs[0].runs[0].text",
    );
    assert_eq!(v, "Changed");
}

#[test]
fn replace_rich_text_formatting() {
    let dir = tmp();
    let out = dir.join("rich.pptx");
    run_ok(&[
        "replace",
        fixture("two_slides.pptx").to_str().unwrap(),
        "--path",
        "slides[0].shapes[0].text_frame.paragraphs[0].runs[0].font.bold",
        "--value",
        "true",
        "--output",
        out.to_str().unwrap(),
    ]);
    run_ok(&[
        "replace",
        out.to_str().unwrap(),
        "--path",
        "slides[0].shapes[0].text_frame.paragraphs[0].runs[0].font.size",
        "--value",
        "2000",
        "--output",
        out.to_str().unwrap(),
    ]);
    let v = query(
        &out,
        "slides[0].shapes[0].text_frame.paragraphs[0].runs[0].font",
    );
    assert_eq!(v["bold"], true);
    assert_eq!(v["size"], 2000);
    let v = query(
        &out,
        "slides[0].shapes[0].text_frame.paragraphs[0].runs[0].text",
    );
    assert_eq!(v, "Alpha", "text preserved through formatting edits");
}

#[test]
fn reorder_slides_via_move() {
    let dir = tmp();
    let out = dir.join("reordered.pptx");
    run_ok(&[
        "move",
        fixture("two_slides.pptx").to_str().unwrap(),
        "--from",
        "slides[1]",
        "--to",
        "slides[0]",
        "--output",
        out.to_str().unwrap(),
    ]);
    let first = query(
        &out,
        "slides[0].shapes[0].text_frame.paragraphs[0].runs[0].text",
    );
    let second = query(
        &out,
        "slides[1].shapes[0].text_frame.paragraphs[0].runs[0].text",
    );
    assert_eq!(first, "Beta");
    assert_eq!(second, "Alpha");
}

#[test]
fn replace_background_roundtrip() {
    let dir = tmp();
    let out = dir.join("bg.pptx");
    run_ok(&[
        "replace",
        fixture("two_slides.pptx").to_str().unwrap(),
        "--path",
        "slides[0].background.fill.color",
        "--value",
        r#""FF00FF""#,
        "--output",
        out.to_str().unwrap(),
    ]);
    let bg = query(&out, "slides[0].background");
    assert_eq!(bg["fill"]["type"], "SOLID");
    assert_eq!(bg["fill"]["color"], "FF00FF");
    let second = query(&out, "slides[1].background");
    assert!(second["fill"]["type"].is_null(), "other slide untouched");
}

#[test]
fn table_cell_replace() {
    let dir = tmp();
    let out = dir.join("tbl.pptx");
    run_ok(&[
        "replace",
        fixture("table_chart.pptx").to_str().unwrap(),
        "--path",
        "slides[0].shapes[1].table.rows[1].cells[0].text",
        "--value",
        r#""Zed""#,
        "--output",
        out.to_str().unwrap(),
    ]);
    let v = query(
        &out,
        "slides[0].shapes[1].table.rows[1].cells[0].text_frame.paragraphs[0].runs[0].text",
    );
    assert_eq!(v, "Zed");
}

#[test]
fn chart_edit_writes_chart_part() {
    let dir = tmp();
    let out = dir.join("chart.pptx");
    run_ok(&[
        "replace",
        fixture("table_chart.pptx").to_str().unwrap(),
        "--path",
        "slides[0].shapes[2].chart.series[0].categories[0]",
        "--value",
        r#""Q3""#,
        "--output",
        out.to_str().unwrap(),
    ]);
    run_ok(&[
        "replace",
        out.to_str().unwrap(),
        "--path",
        "slides[0].shapes[2].chart.series[0].values[1]",
        "--value",
        "30",
        "--output",
        out.to_str().unwrap(),
    ]);
    let chart_xml = read_zip_entry(&out, "ppt/charts/chart1.xml");
    assert!(chart_xml.contains("Q3"), "category updated in chart part");
    assert!(chart_xml.contains(">30<"), "value updated in chart part");
}

#[test]
fn remove_shape() {
    let dir = tmp();
    let out = dir.join("removed.pptx");
    run_ok(&[
        "remove",
        fixture("two_slides.pptx").to_str().unwrap(),
        "--path",
        "slides[0].shapes[0]",
        "--output",
        out.to_str().unwrap(),
    ]);
    let shapes = query(&out, "slides[0].shapes");
    assert_eq!(shapes.as_array().unwrap().len(), 0);
}

#[test]
fn theme_color_replace_writes_theme_part() {
    let dir = tmp();
    let out = dir.join("theme.pptx");
    run_ok(&[
        "replace",
        fixture("two_slides.pptx").to_str().unwrap(),
        "--path",
        "p.theme.colors.accent1",
        "--value",
        r#""FF0000""#,
        "--output",
        out.to_str().unwrap(),
    ]);
    run_ok(&[
        "replace",
        out.to_str().unwrap(),
        "--path",
        "p.theme.fonts.major",
        "--value",
        r#""Arial""#,
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(query(&out, "p.theme.colors.accent1"), "FF0000");
    assert_eq!(query(&out, "p.theme.fonts.major"), "Arial");
    assert_eq!(query(&out, "p.theme.colors.accent2"), "C0504D");
    let theme_xml = read_zip_entry(&out, "ppt/theme/theme1.xml");
    assert!(theme_xml.contains("FF0000"), "color written to theme part");
    assert!(
        theme_xml.contains("typeface=\"Arial\""),
        "font written to theme part"
    );
}

#[test]
fn theme_query_reads_whole_scheme() {
    let v = query(&fixture("two_slides.pptx"), "p.theme.colors");
    let obj = v.as_object().unwrap();
    assert!(obj.contains_key("accent1"));
    assert!(obj.contains_key("dk1"));
}

#[test]
fn master_and_layout_query_expose_shapes() {
    let masters = query(&fixture("two_slides.pptx"), "p.slideMasters");
    let arr = masters.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert!(arr[0]["shapes"].is_array());

    let layouts = query(&fixture("two_slides.pptx"), "p.slideLayouts");
    assert!(!layouts.as_array().unwrap().is_empty());

    let first_layout = query(
        &fixture("two_slides.pptx"),
        "p.slideLayouts[0].shapes[0].name",
    );
    assert!(first_layout.is_string());
}

#[test]
fn master_shape_replace_persists() {
    let dir = tmp();
    let out = dir.join("master.pptx");
    run_ok(&[
        "replace",
        fixture("two_slides.pptx").to_str().unwrap(),
        "--path",
        "p.slideMasters[0].shapes[0].left",
        "--value",
        "100000",
        "--output",
        out.to_str().unwrap(),
    ]);
    assert_eq!(query(&out, "p.slideMasters[0].shapes[0].left"), 100000);
}

#[test]
fn chart_query_reads_series_data() {
    let v = query(&fixture("table_chart.pptx"), "slides[0].shapes[2].chart");
    let obj = v.as_object().unwrap();
    assert_eq!(obj["chart_type"], "c:barChart");
    assert_eq!(obj["series"][0]["name"], "S1");
    assert_eq!(
        obj["series"][0]["categories"],
        serde_json::json!(["Q1", "Q2"])
    );
    assert_eq!(obj["series"][0]["values"], serde_json::json!([10.0, 20.0]));
}

#[test]
fn chart_series_add_and_remove_roundtrip() {
    let dir = tmp();
    let out = dir.join("chart_series.pptx");
    run_ok(&[
        "add",
        fixture("table_chart.pptx").to_str().unwrap(),
        "--path",
        "slides[0].shapes[2].chart.series",
        "--value",
        r#"{"name":"S2","categories":["Q1","Q2"],"values":[30,40]}"#,
        "--output",
        out.to_str().unwrap(),
    ]);
    let series = query(&out, "slides[0].shapes[2].chart.series");
    assert_eq!(series.as_array().unwrap().len(), 2);

    run_ok(&[
        "remove",
        out.to_str().unwrap(),
        "--path",
        "slides[0].shapes[2].chart.series[0]",
        "--output",
        out.to_str().unwrap(),
    ]);
    let series = query(&out, "slides[0].shapes[2].chart.series");
    let arr = series.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["name"], "S2");
}

#[test]
fn table_row_and_column_add_remove_roundtrip() {
    let dir = tmp();
    let out = dir.join("tbl_edit.pptx");
    run_ok(&[
        "add",
        fixture("table_chart.pptx").to_str().unwrap(),
        "--path",
        "slides[0].shapes[1].table.rows",
        "--value",
        r#"{"height":370840,"cells":[{"text_frame":{"paragraphs":[{"runs":[{"text":"New"}]}]}},{}]}"#,
        "--output",
        out.to_str().unwrap(),
    ]);
    let rows = query(&out, "slides[0].shapes[1].table.rows");
    assert_eq!(rows.as_array().unwrap().len(), 3);

    run_ok(&[
        "add",
        out.to_str().unwrap(),
        "--path",
        "slides[0].shapes[1].table.grid",
        "--value",
        r#"{"width":2000000}"#,
        "--output",
        out.to_str().unwrap(),
    ]);
    let tbl = query(&out, "slides[0].shapes[1].table");
    assert_eq!(tbl["grid"].as_array().unwrap().len(), 3);
    assert_eq!(tbl["rows"][0]["cells"].as_array().unwrap().len(), 3);

    run_ok(&[
        "remove",
        out.to_str().unwrap(),
        "--path",
        "slides[0].shapes[1].table.rows[1]",
        "--output",
        out.to_str().unwrap(),
    ]);
    let tbl = query(&out, "slides[0].shapes[1].table");
    assert_eq!(tbl["rows"].as_array().unwrap().len(), 2);

    run_ok(&[
        "remove",
        out.to_str().unwrap(),
        "--path",
        "slides[0].shapes[1].table.grid[1]",
        "--output",
        out.to_str().unwrap(),
    ]);
    let tbl = query(&out, "slides[0].shapes[1].table");
    assert_eq!(tbl["grid"].as_array().unwrap().len(), 2);
    assert_eq!(tbl["rows"][0]["cells"].as_array().unwrap().len(), 2);
}

#[test]
fn add_picture_and_crop_roundtrip() {
    let dir = tmp();
    let png = dir.join("px.png");
    let png_bytes = include_bytes!("fixtures/px.png");
    std::fs::write(&png, png_bytes).unwrap();

    let out = dir.join("pic.pptx");
    let add_value = serde_json::json!({
        "type": "picture",
        "left": 100,
        "top": 200,
        "width": 300,
        "height": 400,
        "image": png.to_string_lossy().to_string(),
    })
    .to_string();
    run_ok(&[
        "add",
        fixture("two_slides.pptx").to_str().unwrap(),
        "--path",
        "slides[0].shapes",
        "--value",
        &add_value,
        "--output",
        out.to_str().unwrap(),
    ]);

    run_ok(&[
        "replace",
        out.to_str().unwrap(),
        "--path",
        "slides[0].shapes[1].crop.left",
        "--value",
        "0.25",
        "--output",
        out.to_str().unwrap(),
    ]);

    let crop = query(&out, "slides[0].shapes[1].crop");
    assert_eq!(crop["left"], 0.25, "crop.left written");
    let img = query(&out, "slides[0].shapes[1].image");
    assert_eq!(img, "image1.png");
    let shape_type = query(&out, "slides[0].shapes[1].shape_type");
    assert_eq!(shape_type, "PICTURE");
}

#[test]
fn replace_fill_color_roundtrip() {
    let dir = tmp();
    let out = dir.join("fill.pptx");
    run_ok(&[
        "replace",
        fixture("two_slides.pptx").to_str().unwrap(),
        "--path",
        "slides[0].shapes[0].fill.color.rgb",
        "--value",
        r#""FF0000""#,
        "--output",
        out.to_str().unwrap(),
    ]);
    let fill = query(&out, "slides[0].shapes[0].fill");
    assert_eq!(fill["type"], "solid");
    assert_eq!(fill["color"]["rgb"], "FF0000");
}

#[test]
fn replace_fill_type_nofill_roundtrip() {
    let dir = tmp();
    let out = dir.join("fill_none.pptx");
    run_ok(&[
        "replace",
        fixture("two_slides.pptx").to_str().unwrap(),
        "--path",
        "slides[0].shapes[0].fill.type",
        "--value",
        r#""no_fill""#,
        "--output",
        out.to_str().unwrap(),
    ]);
    let fill = query(&out, "slides[0].shapes[0].fill");
    assert_eq!(fill["type"], "no_fill");
}

#[test]
fn replace_outline_roundtrip() {
    let dir = tmp();
    let out = dir.join("outline.pptx");
    run_ok(&[
        "replace",
        fixture("two_slides.pptx").to_str().unwrap(),
        "--path",
        "slides[0].shapes[0].outline.width",
        "--value",
        "12700",
        "--output",
        out.to_str().unwrap(),
    ]);
    let outline = query(&out, "slides[0].shapes[0].outline");
    assert_eq!(outline["width"], 12700);
}

#[test]
fn replace_text_frame_rich_roundtrip() {
    let dir = tmp();
    let out = dir.join("rich_frame.pptx");
    run_ok(&[
        "replace",
        fixture("two_slides.pptx").to_str().unwrap(),
        "--path",
        "slides[0].shapes[0].text_frame",
        "--value",
        r#"{"paragraphs":[{"runs":[{"text":"Hi","font":{"bold":true,"size":2000}}]},{"alignment":"CENTER","runs":[{"text":"There"}]}]}"#,
        "--output",
        out.to_str().unwrap(),
    ]);
    let tf = query(&out, "slides[0].shapes[0].text_frame");
    let paras = tf["paragraphs"].as_array().unwrap();
    assert_eq!(paras.len(), 2);
    assert_eq!(paras[0]["runs"][0]["text"], "Hi");
    assert_eq!(paras[0]["runs"][0]["font"]["bold"], true);
    assert_eq!(paras[0]["runs"][0]["font"]["size"], 2000);
    assert_eq!(paras[1]["alignment"], "CENTER");
    assert_eq!(paras[1]["runs"][0]["text"], "There");
}
