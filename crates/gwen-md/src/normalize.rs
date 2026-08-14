use serde_json::{Map, Value};

/// Canonicalize a presentation JSON document so the round-trip property holds:
/// - adjacent runs with identical formatting are merged into a single run;
/// - runs whose text is empty are dropped.
///
/// Applied to both sides of the update diff (so merging never produces edits)
/// and inside `serialize` (so the mirror never carries redundant boundaries).
pub fn normalize(v: &Value) -> Value {
    fn walk(v: &Value) -> Value {
        match v {
            Value::Object(map) => {
                let mut out = Map::new();
                for (k, val) in map {
                    if k == "runs"
                        && let Some(arr) = val.as_array()
                    {
                        out.insert(k.clone(), Value::Array(normalize_runs(arr)));
                        continue;
                    }
                    out.insert(k.clone(), walk(val));
                }
                Value::Object(out)
            }
            Value::Array(arr) => Value::Array(arr.iter().map(walk).collect()),
            other => other.clone(),
        }
    }
    walk(v)
}

fn normalize_runs(runs: &[Value]) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for run in runs {
        let Some(obj) = run.as_object() else {
            continue;
        };
        let text = obj.get("text").and_then(Value::as_str).unwrap_or("");
        if text.is_empty() {
            continue;
        }
        let key = font_key(obj);
        if let Some(last) = out.last_mut()
            && let Some(last_obj) = last.as_object_mut()
            && font_key(last_obj) == key
            && let Some(Value::String(s)) = last_obj.get_mut("text")
        {
            s.push_str(text);
            continue;
        }
        out.push(run.clone());
    }
    out
}

/// The formatting identity of a run: `None` when the run carries no effective
/// font (absent or an empty object), otherwise a canonical string.
fn font_key(run: &Map<String, Value>) -> Option<String> {
    match run.get("font") {
        None | Some(Value::Null) => None,
        Some(Value::Object(m)) if m.is_empty() => None,
        Some(font) => Some(canonical(font)),
    }
}

/// A canonical, order-insensitive string for a JSON value.
pub fn canonical(v: &Value) -> String {
    match v {
        Value::Object(map) => {
            let mut parts: Vec<String> = map
                .iter()
                .map(|(k, val)| format!("{k}={}", canonical(val)))
                .collect();
            parts.sort();
            format!("{{{}}}", parts.join(","))
        }
        Value::Array(arr) => format!(
            "[{}]",
            arr.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        Value::String(s) => format!("\"{s}\""),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn merges_adjacent_same_format_runs() {
        let v = json!({
            "paragraphs": [
                {"runs": [
                    {"text": "a", "font": {"bold": true}},
                    {"text": "b", "font": {"bold": true}},
                ]}
            ]
        });
        let n = normalize(&v);
        let runs = n["paragraphs"][0]["runs"].as_array().unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0]["text"], "ab");
    }

    #[test]
    fn keeps_different_format_runs() {
        let v = json!({
            "paragraphs": [
                {"runs": [
                    {"text": "a", "font": {"bold": true}},
                    {"text": "b", "font": {"italic": true}},
                ]}
            ]
        });
        let n = normalize(&v);
        assert_eq!(n["paragraphs"][0]["runs"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn drops_empty_runs() {
        let v = json!({
            "paragraphs": [
                {"runs": [
                    {"text": ""},
                    {"text": "a"},
                ]}
            ]
        });
        let n = normalize(&v);
        let runs = n["paragraphs"][0]["runs"].as_array().unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0]["text"], "a");
    }

    #[test]
    fn treats_empty_font_as_no_font() {
        let v = json!({
            "paragraphs": [
                {"runs": [
                    {"text": "a", "font": {}},
                    {"text": "b"},
                ]}
            ]
        });
        let n = normalize(&v);
        let runs = n["paragraphs"][0]["runs"].as_array().unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0]["text"], "ab");
    }

    #[test]
    fn normalizes_table_cell_runs() {
        let v = json!({
            "table": {"rows": [{"cells": [{"text_frame": {"paragraphs": [
                {"runs": [{"text": "x"}, {"text": "y"}]}
            ]}}]}]}
        });
        let n = normalize(&v);
        let runs = n["table"]["rows"][0]["cells"][0]["text_frame"]["paragraphs"][0]["runs"]
            .as_array()
            .unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0]["text"], "xy");
    }
}
