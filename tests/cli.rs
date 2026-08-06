use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::{Value, json};

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

/// Navigate a dotted/bracketed path (as `p`-prefixed, e.g. `slides[0].shapes`)
/// through a parsed jsonfy snapshot.
fn navigate<'a>(value: &'a Value, path: &str) -> &'a Value {
    let mut cur = value;
    let mut rest = path.strip_prefix('p').unwrap_or(path);
    rest = rest.strip_prefix('.').unwrap_or(rest);
    while !rest.is_empty() {
        if let Some(rest_after_idx) = rest.strip_prefix('[') {
            let close = rest_after_idx.find(']').expect("unclosed index in path");
            let idx: usize = rest_after_idx[..close]
                .parse()
                .unwrap_or_else(|_| panic!("invalid index in path: {path}"));
            cur = &cur[idx];
            rest = rest_after_idx[close + 1..]
                .strip_prefix('.')
                .unwrap_or(&rest_after_idx[close + 1..]);
        } else {
            let end = rest.find(['.', '[']).unwrap_or(rest.len());
            cur = &cur[&rest[..end]];
            rest = &rest[end..];
            rest = rest.strip_prefix('.').unwrap_or(rest);
        }
    }
    cur
}

fn query(input: &Path, path: &str) -> Value {
    let out = run_ok(&["jsonfy", "--input", input.to_str().unwrap()]);
    let root: Value = serde_json::from_str(&out).unwrap();
    navigate(&root, path).clone()
}

/// Write `edits` to `dir/edits.json` and apply it to `input`.
fn update(input: &Path, edits: &Value) -> PathBuf {
    let out = run_ok(&["jsonfy", "--input", input.to_str().unwrap()]);
    let snapshot: Value = serde_json::from_str(&out).unwrap();
    update_snapshot(input, &overlay_merge(&snapshot, edits))
}

/// Apply a full snapshot document (in the shape of jsonfy output) to `input`.
fn update_snapshot(input: &Path, snapshot: &Value) -> PathBuf {
    let dir = tmp();
    let json_file = dir.join("deck.json");
    std::fs::write(&json_file, serde_json::to_string(snapshot).unwrap()).unwrap();
    let output = dir.join("out.pptx");
    run_ok(&[
        "update",
        "--input",
        input.to_str().unwrap(),
        "--json",
        json_file.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
    ]);
    output
}

/// Fold a partial overlay into a snapshot document, replicating the old
/// partial-update semantics: null deletes, objects merge by key, arrays are
/// positional (null elements drop, elements beyond the overlay drop).
fn overlay_merge(cur: &Value, overlay: &Value) -> Value {
    match (cur, overlay) {
        (Value::Object(c), Value::Object(o)) => {
            let mut out = c.clone();
            for (k, v) in o {
                if v.is_null() {
                    out.remove(k);
                } else {
                    out.insert(
                        k.clone(),
                        overlay_merge(c.get(k).unwrap_or(&Value::Null), v),
                    );
                }
            }
            Value::Object(out)
        }
        (Value::Array(c), Value::Array(o)) => {
            let mut out = Vec::new();
            for (i, v) in o.iter().enumerate() {
                if v.is_null() {
                    out.push(Value::Null);
                    continue;
                }
                let base = c.get(i).cloned().unwrap_or(Value::Null);
                out.push(overlay_merge(&base, v));
            }
            Value::Array(out)
        }
        _ => overlay.clone(),
    }
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
fn jsonfy_template_snapshot_is_wide() {
    let pres = query(&fixture("template.pptx"), "p");
    assert_eq!(pres["slide_width"], 12192000);
    assert_eq!(pres["slide_height"], 6858000);
    assert_eq!(pres["slides"].as_array().unwrap().len(), 1);
    assert_eq!(pres["slides"][0]["shapes"].as_array().unwrap().len(), 0);
    assert!(pres["slides"][0]["background"]["fill"]["type"].is_null());
}

#[test]
fn jsonfy_template_43_snapshot_is_standard() {
    let pres = query(&fixture("template_43.pptx"), "p");
    assert_eq!(pres["slide_width"], 9144000);
    assert_eq!(pres["slide_height"], 6858000);
}

const PNG_1PX: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00\x1f\x15\xc4\x89\x00\x00\x00\x0dIDATx\xda\x63\xf8\xcf\xc0\xf0\x1f\x00\x05\x00\x01\xff\xff\xff\x14\xbb\x00\x00\x00\x00IEND\xaeB`\x82";

/// Rebuild the template fixture as a zip with an injected picture shape and
/// image part, so media extraction can be exercised end to end.
fn inject_image_into_template(deck: &Path) {
    let file = std::fs::File::open(fixture("template.pptx")).unwrap();
    let mut ar = zip::ZipArchive::new(file).unwrap();
    let names: Vec<String> = (0..ar.len())
        .map(|i| ar.by_index(i).unwrap().name().to_string())
        .collect();
    let out = std::fs::File::create(deck).unwrap();
    let mut zw = zip::ZipWriter::new(out);
    let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
    for name in names {
        let mut entry = ar.by_name(&name).unwrap();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        zw.start_file(&name, opts).unwrap();
        match name.as_str() {
            "ppt/slides/slide1.xml" => {
                let s = String::from_utf8(data).unwrap();
                let pic = "<p:pic><p:nvPicPr><p:cNvPr id=\"21\" name=\"Pic1\"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed=\"rId9\"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x=\"100000\" y=\"100000\"/><a:ext cx=\"500000\" cy=\"400000\"/></a:xfrm><a:prstGeom prst=\"rect\"/></p:spPr></p:pic>";
                zw.write_all(
                    s.replace("</p:spTree>", &format!("{pic}</p:spTree>"))
                        .as_bytes(),
                )
                .unwrap();
            }
            "ppt/slides/_rels/slide1.xml.rels" => {
                let rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/><Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image1.png"/></Relationships>"#;
                zw.write_all(rels.as_bytes()).unwrap();
            }
            _ => zw.write_all(&data).unwrap(),
        }
    }
    zw.start_file("ppt/media/image1.png", opts).unwrap();
    zw.write_all(PNG_1PX).unwrap();
    zw.finish().unwrap();
}

#[test]
fn jsonfy_extracts_media_with_media_flag() {
    let dir = tmp();
    let deck = dir.join("deck.pptx");
    inject_image_into_template(&deck);

    let media_dir = dir.join("media");
    run_ok(&[
        "jsonfy",
        "--input",
        deck.to_str().unwrap(),
        "--media",
        media_dir.to_str().unwrap(),
    ]);

    let extracted = media_dir.join("image1.png");
    assert!(extracted.exists(), "image extracted into media dir");
    assert_eq!(
        std::fs::read(&extracted).unwrap(),
        PNG_1PX,
        "extracted bytes match the embedded image"
    );
    let snapshot: Value =
        serde_json::from_str(&run_ok(&["jsonfy", "--input", deck.to_str().unwrap()])).unwrap();
    assert_eq!(snapshot["slides"][0]["shapes"][0]["image"], "image1.png");
}

#[test]
fn update_core_properties_roundtrip() {
    let out = update(
        &fixture("template.pptx"),
        &json!({ "core_properties": { "title": "My Deck" } }),
    );
    let pres = query(&out, "p");
    assert_eq!(pres["core_properties"]["title"], "My Deck");
}

#[test]
fn update_run_text_roundtrip() {
    let out = update(
        &fixture("two_slides.pptx"),
        &json!({
            "slides": [
                {"shapes": [{"text_frame": {
                    "paragraphs": [{"runs": [{"text": "Changed"}]}]
                }}]},
                {}
            ]
        }),
    );
    let v = query(
        &out,
        "slides[0].shapes[0].text_frame.paragraphs[0].runs[0].text",
    );
    assert_eq!(v, "Changed");
    // The second slide is untouched.
    let v = query(
        &out,
        "slides[1].shapes[0].text_frame.paragraphs[0].runs[0].text",
    );
    assert_eq!(v, "Beta");
}

#[test]
fn update_rich_text_formatting() {
    let out = update(
        &fixture("two_slides.pptx"),
        &json!({
            "slides": [
                {"shapes": [{"text_frame": {
                    "paragraphs": [{"runs": [{"text": "Alpha", "font": {"bold": true, "size": 2000}}]}]
                }}]},
                {}
            ]
        }),
    );
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
fn update_whole_paragraph_replace() {
    // The original pain point: replace text_frame.paragraphs[k] wholesale.
    let out = update(
        &fixture("two_slides.pptx"),
        &json!({
            "slides": [
                {"shapes": [{"text_frame": {
                    "paragraphs": [
                        {"runs": [{"text": "Hi", "font": {"bold": true, "size": 2000}}]},
                        {"alignment": "CENTER", "runs": [{"text": "There"}]}
                    ]
                }}]},
                {}
            ]
        }),
    );
    let tf = query(&out, "slides[0].shapes[0].text_frame");
    let paras = tf["paragraphs"].as_array().unwrap();
    assert_eq!(paras.len(), 2);
    assert_eq!(paras[0]["runs"][0]["text"], "Hi");
    assert_eq!(paras[0]["runs"][0]["font"]["bold"], true);
    assert_eq!(paras[0]["runs"][0]["font"]["size"], 2000);
    assert_eq!(paras[1]["alignment"], "CENTER");
    assert_eq!(paras[1]["runs"][0]["text"], "There");
}

#[test]
fn update_delete_paragraph() {
    let out = update(
        &fixture("two_slides.pptx"),
        &json!({
            "slides": [
                {"shapes": [{"text_frame": {
                    "paragraphs": [{}, null]
                }}]},
                {}
            ]
        }),
    );
    let paras = query(&out, "slides[0].shapes[0].text_frame.paragraphs");
    assert_eq!(paras.as_array().unwrap().len(), 1);
}

#[test]
fn update_append_paragraph() {
    let out = update(
        &fixture("two_slides.pptx"),
        &json!({
            "slides": [
                {"shapes": [{"text_frame": {
                    "paragraphs": [
                        {},
                        {"runs": [{"text": "Appended"}]}
                    ]
                }}]},
                {}
            ]
        }),
    );
    let v = query(
        &out,
        "slides[0].shapes[0].text_frame.paragraphs[1].runs[0].text",
    );
    assert_eq!(v, "Appended");
}

#[test]
fn update_background_roundtrip() {
    let out = update(
        &fixture("two_slides.pptx"),
        &json!({
            "slides": [
                {"background": {"fill": {"color": "FF00FF"}}},
                {}
            ]
        }),
    );
    let bg = query(&out, "slides[0].background");
    assert_eq!(bg["fill"]["type"], "SOLID");
    assert_eq!(bg["fill"]["color"], "FF00FF");
    let second = query(&out, "slides[1].background");
    assert!(second["fill"]["type"].is_null(), "other slide untouched");
}

#[test]
fn update_table_cell_text() {
    let out = update(
        &fixture("table_chart.pptx"),
        &json!({
            "slides": [{"shapes": [{}, {"table": {"rows": [{}, {"cells": [{}, {
                "text_frame": {"paragraphs": [{"runs": [{"text": "Zed"}]}]}
            }]}]}}, {}]}]
        }),
    );
    let v = query(
        &out,
        "slides[0].shapes[1].table.rows[1].cells[1].text_frame.paragraphs[0].runs[0].text",
    );
    assert_eq!(v, "Zed");
}

#[test]
fn update_chart_series_data() {
    let out = update(
        &fixture("table_chart.pptx"),
        &json!({
            "slides": [{"shapes": [{}, {}, {"chart": {"series": [
                {"categories": ["Q3", "Q2"], "values": [30, 20.0]}
            ]}}]}]
        }),
    );
    assert_eq!(
        query(&out, "slides[0].shapes[2].chart.series[0].categories[0]"),
        "Q3"
    );
    assert_eq!(
        query(&out, "slides[0].shapes[2].chart.series[0].values[0]"),
        30.0
    );
    let chart_xml = read_zip_entry(&out, "ppt/charts/chart1.xml");
    assert!(chart_xml.contains("Q3"), "category updated in chart part");
    assert!(chart_xml.contains(">30<"), "value updated in chart part");
}

#[test]
fn update_delete_shape() {
    let out = update(
        &fixture("two_slides.pptx"),
        &json!({ "slides": [{ "shapes": [null] }, {}] }),
    );
    let shapes = query(&out, "slides[0].shapes");
    assert_eq!(shapes.as_array().unwrap().len(), 0);
}

#[test]
fn update_theme_roundtrip() {
    let out = update(
        &fixture("two_slides.pptx"),
        &json!({
            "theme": {
                "colors": {"accent1": "FF0000"},
                "fonts": {"major": "Arial"}
            }
        }),
    );
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
fn update_delete_theme_color_by_removing_key_errors() {
    let mut snapshot: Value = serde_json::from_str(&run_ok(&[
        "jsonfy",
        "--input",
        fixture("two_slides.pptx").to_str().unwrap(),
    ]))
    .unwrap();
    snapshot["theme"]["colors"]
        .as_object_mut()
        .unwrap()
        .remove("accent1");

    let dir = tmp();
    let json_file = dir.join("deck.json");
    std::fs::write(&json_file, serde_json::to_string(&snapshot).unwrap()).unwrap();
    let status = Command::new(bin())
        .args([
            "update",
            "--input",
            fixture("two_slides.pptx").to_str().unwrap(),
            "--json",
            json_file.to_str().unwrap(),
            "--output",
            dir.join("out.pptx").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!status.status.success());
    let stderr = String::from_utf8_lossy(&status.stderr);
    assert!(
        stderr.contains("can only be set"),
        "unsupported removal must error loudly, got: {stderr}"
    );
}

#[test]
fn update_delete_text_frame_by_removing_key() {
    let mut snapshot: Value = serde_json::from_str(&run_ok(&[
        "jsonfy",
        "--input",
        fixture("two_slides.pptx").to_str().unwrap(),
    ]))
    .unwrap();
    snapshot["slides"][0]["shapes"][0]
        .as_object_mut()
        .unwrap()
        .remove("text_frame");

    let out = update_snapshot(&fixture("two_slides.pptx"), &snapshot);
    let slide_xml = read_zip_entry(&out, "ppt/slides/slide1.xml");
    assert!(
        !slide_xml.contains("<p:txBody"),
        "removed text_frame must delete the shape's txBody"
    );
    let has_tf = query(&out, "slides[0].shapes[0].has_text_frame");
    assert_eq!(has_tf, false);
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
    let masters = query(&fixture("two_slides.pptx"), "p.slide_masters");
    let arr = masters.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert!(arr[0]["shapes"].is_array());
    assert!(arr[0]["slide_layouts"].is_array());

    let first_layout_name = query(
        &fixture("two_slides.pptx"),
        "p.slide_masters[0].slide_layouts[0].name",
    );
    assert!(first_layout_name.is_string());

    let first_layout_shape = query(
        &fixture("two_slides.pptx"),
        "p.slide_masters[0].slide_layouts[0].shapes[0].name",
    );
    assert!(first_layout_shape.is_string());
}

#[test]
fn slide_layout_reference_points_to_master_layout() {
    let reference = query(&fixture("two_slides.pptx"), "slides[0].slide_layout");
    let m = reference["master"].as_u64().unwrap() as usize;
    let l = reference["layout"].as_u64().unwrap() as usize;
    let expected = query(
        &fixture("two_slides.pptx"),
        &format!("p.slide_masters[{m}].slide_layouts[{l}].name"),
    );
    assert_eq!(reference["name"], expected);
}

#[test]
fn update_master_shape_persists() {
    let out = update(
        &fixture("two_slides.pptx"),
        &json!({ "slide_masters": [{ "shapes": [{ "left": 100000 }] }] }),
    );
    assert_eq!(query(&out, "p.slide_masters[0].shapes[0].left"), 100000);
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
fn update_chart_series_append_and_delete() {
    let appended = update(
        &fixture("table_chart.pptx"),
        &json!({
            "slides": [{"shapes": [{}, {}, {"chart": {"series": [
                {},
                {"name": "S2", "categories": ["Q1", "Q2"], "values": [30.0, 40.0]}
            ]}}]}]
        }),
    );
    let series = query(&appended, "slides[0].shapes[2].chart.series");
    assert_eq!(series.as_array().unwrap().len(), 2);
    assert_eq!(series[1]["name"], "S2");

    let deleted = update(
        &appended,
        &json!({
            "slides": [{"shapes": [{}, {}, {"chart": {"series": [
                null, {"name": "S2", "categories": ["Q1", "Q2"], "values": [30.0, 40.0]}
            ]}}]}]
        }),
    );
    let series = query(&deleted, "slides[0].shapes[2].chart.series");
    let arr = series.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["name"], "S2");
}

#[test]
fn update_table_row_and_column_add_remove_roundtrip() {
    let appended = update(
        &fixture("table_chart.pptx"),
        &json!({
            "slides": [{"shapes": [{}, {"table": {"rows": [
                {},
                {},
                {"height": 370840, "cells": [
                    {"text_frame": {"paragraphs": [{"runs": [{"text": "New"}]}]}},
                    {}
                ]}
            ]}}, {}]}]
        }),
    );
    let rows = query(&appended, "slides[0].shapes[1].table.rows");
    assert_eq!(rows.as_array().unwrap().len(), 3);

    let removed = update(
        &appended,
        &json!({
            "slides": [{"shapes": [{}, {"table": {"rows": [
                {}, null, {"height": 370840, "cells": [
                    {"text_frame": {"paragraphs": [{"runs": [{"text": "New"}]}]}},
                    {}
                ]}
            ]}}, {}]}]
        }),
    );
    let rows = query(&removed, "slides[0].shapes[1].table.rows");
    assert_eq!(rows.as_array().unwrap().len(), 2);
}

#[test]
fn update_append_shape() {
    let out = update(
        &fixture("two_slides.pptx"),
        &json!({
            "slides": [
                {"shapes": [
                    {},
                    {
                        "shape_id": 99,
                        "name": "New",
                        "shape_type": "TEXT_BOX",
                        "left": 100000,
                        "top": 100000,
                        "width": 5000000,
                        "height": 500000,
                        "is_placeholder": false,
                        "has_text_frame": true,
                        "text_frame": {"paragraphs": [{"runs": [{"text": "Hi"}]}]}
                    }
                ]},
                {}
            ]
        }),
    );
    let shapes = query(&out, "slides[0].shapes");
    assert_eq!(shapes.as_array().unwrap().len(), 2);
    assert_eq!(shapes[1]["name"], "New");
    assert_eq!(
        shapes[1]["text_frame"]["paragraphs"][0]["runs"][0]["text"],
        "Hi"
    );
}

#[test]
fn update_notes_slide_create_and_delete() {
    let created = update(
        &fixture("two_slides.pptx"),
        &json!({
            "slides": [
                {"notes": {"shapes": [{
                    "shape_id": 1,
                    "name": "Notes",
                    "shape_type": "TEXT_BOX",
                    "left": 100,
                    "top": 100,
                    "width": 500,
                    "height": 300,
                    "is_placeholder": false,
                    "has_text_frame": true,
                    "text_frame": {"paragraphs": [{"runs": [{"text": "Notes!"}]}]}
                }]}},
                {}
            ]
        }),
    );
    let v = query(
        &created,
        "slides[0].notes.shapes[0].text_frame.paragraphs[0].runs[0].text",
    );
    assert_eq!(v, "Notes!");

    let deleted = update(&created, &json!({ "slides": [{ "notes": null }, {}] }));
    assert!(query(&deleted, "slides[0].notes").is_null());
}

#[test]
fn update_readonly_field_errors() {
    let dir = tmp();
    let edits = dir.join("edits.json");
    std::fs::write(
        &edits,
        r#"{"slides": [{"shapes": [{"shape_id": 999999}]}]}"#,
    )
    .unwrap();
    let out = Command::new(bin())
        .args([
            "update",
            "--input",
            fixture("two_slides.pptx").to_str().unwrap(),
            "--json",
            edits.to_str().unwrap(),
            "--output",
            dir.join("o.pptx").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!out.status.success(), "read-only change must error loudly");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("shape_id") && stderr.contains("read-only"),
        "error should call out the read-only field: {stderr}"
    );
}

#[test]
fn update_unknown_field_errors() {
    let dir = tmp();
    let edits = dir.join("edits.json");
    std::fs::write(&edits, r#"{"slides": [{"shapes": [{"bogus_field": 1}]}]}"#).unwrap();
    let out = Command::new(bin())
        .args([
            "update",
            "--input",
            fixture("two_slides.pptx").to_str().unwrap(),
            "--json",
            edits.to_str().unwrap(),
            "--output",
            dir.join("o.pptx").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!out.status.success(), "unknown field must error loudly");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("bogus_field"),
        "error should mention the offending field: {stderr}"
    );
}

#[test]
fn placeholder_inherits_geometry_and_style() {
    let shape = query(&fixture("placeholder.pptx"), "slides[0].shapes[0]");
    // Geometry inherited from the slide layout.
    assert_eq!(shape["left"], 685800);
    assert_eq!(shape["top"], 2130425);
    assert_eq!(shape["width"], 7772400);
    assert_eq!(shape["height"], 1470025);
    // Fill inherited from the layout spPr.
    assert_eq!(shape["fill"]["type"], "solid");
    assert_eq!(shape["fill"]["color"]["rgb"], "C7000B");
    // Text defaults inherited from the layout lstStyle.
    let dps = &shape["text_frame"]["default_paragraph_style"];
    assert_eq!(dps["alignment"], "CENTER");
    assert_eq!(dps["font"]["name"], "Calibri");
    assert_eq!(dps["font"]["size"], 3200);
    assert_eq!(dps["font"]["bold"], true);
    assert_eq!(dps["font"]["color"]["theme_color"], "tx1");
}

#[test]
fn update_default_paragraph_style_roundtrips() {
    let dps = query(
        &fixture("placeholder.pptx"),
        "slides[0].shapes[0].text_frame.default_paragraph_style",
    );
    let out = update(
        &fixture("placeholder.pptx"),
        &json!({
            "slides": [{"shapes": [{
                "text_frame": {"default_paragraph_style": {
                    "alignment": "LEFT",
                    "font": {"name": "Arial", "size": 2800, "bold": false}
                }}
            }]}]
        }),
    );
    let updated = query(
        &out,
        "slides[0].shapes[0].text_frame.default_paragraph_style",
    );
    assert_eq!(updated["alignment"], "LEFT");
    assert_eq!(updated["font"]["name"], "Arial");
    assert_eq!(updated["font"]["size"], 2800);
    assert_eq!(updated["font"]["bold"], false);
    let _ = dps;
}

#[test]
fn update_does_not_touch_unmentioned_fields() {
    // Hand back the full deck snapshot with one field changed; everything else
    // must be byte-identical.
    let dir = tmp();
    let deck = dir.join("deck.pptx");
    std::fs::copy(fixture("two_slides.pptx"), &deck).unwrap();
    let snapshot: Value =
        serde_json::from_str(&run_ok(&["jsonfy", "--input", deck.to_str().unwrap()])).unwrap();
    let mut edits = snapshot.clone();
    edits["slides"][0]["shapes"][0]["text_frame"]["paragraphs"][0]["runs"][0]["text"] =
        json!("Edited");

    let out = update_snapshot(&deck, &edits);
    let before = snapshot["slides"][1].clone();
    let after = query(&out, "slides[1]");
    assert_eq!(before, after, "untouched slide preserved exactly");
}
