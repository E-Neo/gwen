use std::path::Path;

use gwen_md::{parse, serialize};
use gwen_pptx::engine::{query, readonly};
use gwen_pptx::opc::Package;
use serde_json::Value;

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn snapshot(path: &Path) -> Value {
    let pkg = Package::open(path).expect("open package");
    query::query_document(&pkg, None).expect("query snapshot")
}

fn assert_roundtrip(fx: &str) {
    let s = snapshot(&fixture(fx));
    let md = serialize::serialize(&s);
    let reparsed = parse::parse(&md);
    let expected = readonly::project(&s);
    assert_eq!(
        canonical(&reparsed),
        canonical(&expected),
        "round-trip mismatch for {fx}\n--- markdown ---\n{md}"
    );
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

#[test]
fn template_roundtrips() {
    assert_roundtrip("template.pptx");
}

#[test]
fn template_43_roundtrips() {
    assert_roundtrip("template_43.pptx");
}

#[test]
fn two_slides_roundtrips() {
    assert_roundtrip("two_slides.pptx");
}

#[test]
fn table_chart_roundtrips() {
    assert_roundtrip("table_chart.pptx");
}

#[test]
fn placeholder_roundtrips() {
    assert_roundtrip("placeholder.pptx");
}
