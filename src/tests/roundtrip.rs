use std::path::{Path, PathBuf};

use gwen_md::{normalize, read_document, write_document};
use gwen_pptx::engine::query;
use gwen_pptx::opc::Package;
use serde_json::Value;

#[path = "decks.rs"]
mod decks;

#[path = "support.rs"]
mod support;

fn fixture(name: &str) -> PathBuf {
    decks::deck(name)
}

fn snapshot(path: &Path) -> Value {
    let pkg = Package::open(path).expect("open package");
    query::query_document(&pkg, None).expect("query snapshot")
}

/// Recursively sort object keys so the comparison is insensitive to the
/// insertion order each side builds its maps in.
fn canonical(v: &Value) -> Value {
    match v {
        Value::Object(map) => {
            let mut sorted: Vec<(String, Value)> = map
                .iter()
                .map(|(k, val)| (k.clone(), canonical(val)))
                .collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(sorted.into_iter().collect())
        }
        Value::Array(arr) => Value::Array(arr.iter().map(canonical).collect()),
        other => other.clone(),
    }
}

/// Drop the build-time detail the complete mirror deliberately regenerates:
/// `chart.r_id` is re-assigned to a fresh relationship on every build; shape
/// `has_text_frame`/`is_placeholder` are derived from the marker (a placeholder
/// marker means `true`, its absence `false`); and table cells mirror only cell
/// text (paragraph properties like `level` are not part of the table form).
fn strip_rebuild_time(v: &Value) -> Value {
    match v {
        Value::Object(map) => {
            let sheet_type = map
                .get("shape_type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let mut m = map.clone();
            if sheet_type == "CHART"
                && let Some(chart) = m.get_mut("chart").and_then(Value::as_object_mut)
            {
                chart.remove("r_id");
            }
            m.remove("has_text_frame");
            m.remove("is_placeholder");
            if sheet_type == "TABLE" {
                let paras = m
                    .get_mut("table")
                    .and_then(Value::as_object_mut)
                    .and_then(|t| t.get_mut("rows"))
                    .and_then(Value::as_array_mut);
                if let Some(rows) = paras {
                    for row in rows {
                        let cells = row
                            .as_object_mut()
                            .and_then(|r| r.get_mut("cells"))
                            .and_then(Value::as_array_mut);
                        if let Some(cells) = cells {
                            for cell in cells {
                                let p = cell
                                    .as_object_mut()
                                    .and_then(|c| c.get_mut("text_frame"))
                                    .and_then(Value::as_object_mut)
                                    .and_then(|t| t.get_mut("paragraphs"))
                                    .and_then(Value::as_array_mut);
                                if let Some(paras) = p {
                                    for para in paras.iter_mut() {
                                        if let Some(po) = para.as_object_mut() {
                                            let runs = po.get("runs").cloned();
                                            *po = serde_json::Map::new();
                                            if let Some(r) = runs {
                                                po.insert("runs".into(), r);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Value::Object(
                m.into_iter()
                    .map(|(k, val)| (k, strip_rebuild_time(&val)))
                    .collect(),
            )
        }
        Value::Array(arr) => Value::Array(arr.iter().map(strip_rebuild_time).collect()),
        other => other.clone(),
    }
}

/// The multi-file complete mirror must reproduce the unprojected snapshot: the
/// `PRESENTATION.md`/`src/` tree round-trips through `write_document` and
/// `read_document` back to the exact query value.
fn assert_multi_file_roundtrip(fx: &str) {
    let s = snapshot(&fixture(fx));
    let dir = std::env::temp_dir().join(format!(
        "gwen-mirror-{}-{}",
        std::process::id(),
        fx.replace(['/', '.'], "_")
    ));
    let _ = std::fs::remove_dir_all(&dir);
    write_document(&s, &dir).expect("write mirror");
    let back = read_document(&dir).expect("read mirror");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        canonical(&normalize::normalize(&strip_rebuild_time(&back))),
        canonical(&normalize::normalize(&strip_rebuild_time(&s))),
        "multi-file mirror mismatch for {fx}"
    );
}

#[test]
fn multi_file_mirror_template() {
    assert_multi_file_roundtrip("template.pptx");
}

#[test]
fn multi_file_mirror_template_43() {
    assert_multi_file_roundtrip("template_43.pptx");
}

#[test]
fn multi_file_mirror_two_slides() {
    assert_multi_file_roundtrip("two_slides.pptx");
}

#[test]
fn multi_file_mirror_table_chart() {
    assert_multi_file_roundtrip("table_chart.pptx");
}

#[test]
fn multi_file_mirror_placeholder() {
    assert_multi_file_roundtrip("placeholder.pptx");
}

#[test]
fn multi_file_mirror_notes_placeholder() {
    assert_multi_file_roundtrip("notes_placeholder.pptx");
}
