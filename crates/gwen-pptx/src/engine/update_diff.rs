use serde_json::Value;

use crate::engine::readonly;
use crate::path::PathSegment;

/// Fields that are replaced as a whole rather than diffed field-by-field. The
/// DTO models them as a single document so it makes no sense to patch their
/// inner leaves.
const ATOMIC_FIELDS: [&str; 1] = ["default_paragraph_style"];

/// Array kinds whose elements are matched across the two documents by content
/// fingerprint rather than by position. Ordered matching keeps untouched
/// elements byte-for-byte (their unmodeled XML survives) when siblings are
/// inserted or removed around them.

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EditOp {
    /// Replace the node at `path` with `value`.
    Set,
    /// Remove the node at `path` (element, property, or attribute).
    Delete,
    /// Append `value` to the array at `path` (only ever produced for an index
    /// one past the current end, so it is always an append).
    Insert,
    /// Regenerate a slide at `path` in place (`slides[i]`): the part keeps its
    /// URI, layout relationship, notes and slide id; only its content is
    /// rebuilt from `value`.
    Replace,
}

#[derive(Debug, Clone)]
pub struct Edit {
    pub path: Vec<PathSegment>,
    pub op: EditOp,
    pub value: Option<Value>,
}

/// Deeply compare the current JSON projection of the deck against the edited
/// document and emit the minimal set of edits needed to make the deck match.
///
/// Semantics:
/// - Objects are full-state snapshots: a key present in `current` but absent
///   from `new` is deleted, a key absent from `current` but present in `new`
///   is added, and `null` also deletes. Read-only fields (see
///   `engine::readonly`) are never edited: when absent from `new` they are
///   left untouched.
/// - Arrays of slides, shapes, paragraphs, runs and grid columns are matched
///   element-by-element by content fingerprint in document order, so an
///   insertion or removal does not rewrite untouched siblings. Other arrays
///   are positional full assertions.
/// - An inserted element targets its index in the new document; the apply
///   engine inserts at that position.
/// - Identical leaf values produce no edit.
pub fn diff(current: &Value, new: &Value) -> Vec<Edit> {
    let mut out = Vec::new();
    diff_rec(&mut out, &[], current, new);
    out
}

fn diff_rec(out: &mut Vec<Edit>, path: &[PathSegment], cur: &Value, new: &Value) {
    match (cur, new) {
        (Value::Object(cur_map), Value::Object(new_map)) => {
            for (key, new_val) in new_map {
                if !cur_map.contains_key(key) && !new_val.is_null() {
                    let mut p = path.to_vec();
                    p.push(PathSegment::Field(key.clone()));
                    out.push(Edit {
                        path: p,
                        op: EditOp::Set,
                        value: Some(new_val.clone()),
                    });
                }
            }
            for (key, cur_val) in cur_map {
                let mut p = path.to_vec();
                p.push(PathSegment::Field(key.clone()));
                match new_map.get(key) {
                    None => {
                        if readonly::skip_key(path, key) {
                            continue;
                        }
                        out.push(Edit {
                            path: p,
                            op: EditOp::Delete,
                            value: None,
                        })
                    }
                    Some(new_val) => {
                        if new_val.is_null() {
                            if !cur_val.is_null() {
                                out.push(Edit {
                                    path: p,
                                    op: EditOp::Delete,
                                    value: None,
                                });
                            }
                        } else if cur_val.is_null()
                            || (ATOMIC_FIELDS.contains(&key.as_str()) && cur_val != new_val)
                        {
                            out.push(Edit {
                                path: p,
                                op: EditOp::Set,
                                value: Some(new_val.clone()),
                            });
                        } else if cur_val.is_object() && new_val.is_object() {
                            diff_rec(out, &p, cur_val, new_val);
                        } else if cur_val.is_array() && new_val.is_array() {
                            diff_array(out, &p, cur_val, new_val);
                        } else if cur_val != new_val {
                            out.push(Edit {
                                path: p,
                                op: EditOp::Set,
                                value: Some(new_val.clone()),
                            });
                        }
                    }
                }
            }
        }
        (Value::Array(_), Value::Array(_)) => diff_array(out, path, cur, new),
        _ => {
            if cur != new {
                out.push(Edit {
                    path: path.to_vec(),
                    op: EditOp::Set,
                    value: Some(new.clone()),
                });
            }
        }
    }
}

fn diff_array(out: &mut Vec<Edit>, path: &[PathSegment], cur: &Value, new: &Value) {
    let cur_arr = cur.as_array().expect("guarded by caller");
    let new_arr = new.as_array().expect("guarded by caller");
    let Some(kind) = array_kind(path) else {
        diff_array_positional(out, path, cur_arr, new_arr);
        return;
    };

    // Longest-common-subsequence alignment on content fingerprints: pairs
    // keep untouched siblings matched across insertions and removals, and
    // produce edits that target only the elements that actually changed.
    let cur_fp: Vec<String> = cur_arr.iter().map(|v| fingerprint(kind, v)).collect();
    let new_fp: Vec<String> = new_arr.iter().map(|v| fingerprint(kind, v)).collect();
    let (pairs, unmatched_cur, unmatched_new) = match_sequences(&cur_fp, &new_fp);

    for (ci, ni) in pairs {
        let mut p = path.to_vec();
        p.push(PathSegment::Index(ci));
        let cur_val = &cur_arr[ci];
        let new_val = &new_arr[ni];
        if cur_val.is_object() && new_val.is_object() {
            diff_rec(out, &p, cur_val, new_val);
        } else if new_val.is_array() {
            diff_array(out, &p, cur_val, new_val);
        } else if cur_val != new_val {
            out.push(Edit {
                path: p,
                op: EditOp::Set,
                value: Some(new_val.clone()),
            });
        }
    }
    // A slide whose shape signature changed (a shape added or removed) is
    // unmatched on both sides. When it was edited at the same index it was at,
    // regenerate it in place: a `Replace` keeps the part URI, layout
    // relationship, notes and slide id, so a shape edit never churns part
    // names or reassigns layouts. Genuine insertions and removals stay
    // Insert/Delete (reorders fall out as delete+insert, documented loss).
    let (unmatched_cur, unmatched_new) = if kind == "slides" {
        for ni in &unmatched_new {
            if unmatched_cur.contains(ni) {
                let mut p = path.to_vec();
                p.push(PathSegment::Index(*ni));
                out.push(Edit {
                    path: p,
                    op: EditOp::Replace,
                    value: Some(new_arr[*ni].clone()),
                });
            }
        }
        let paired: Vec<usize> = unmatched_new
            .iter()
            .copied()
            .filter(|ni| unmatched_cur.contains(ni))
            .collect();
        let unmatched_cur = unmatched_cur
            .into_iter()
            .filter(|ci| !paired.contains(ci))
            .collect();
        let unmatched_new = unmatched_new
            .into_iter()
            .filter(|ni| !paired.contains(ni))
            .collect();
        (unmatched_cur, unmatched_new)
    } else {
        (unmatched_cur, unmatched_new)
    };
    for ci in unmatched_cur {
        let mut p = path.to_vec();
        p.push(PathSegment::Index(ci));
        out.push(Edit {
            path: p,
            op: EditOp::Delete,
            value: None,
        });
    }
    for ni in unmatched_new {
        let mut p = path.to_vec();
        p.push(PathSegment::Index(ni));
        let value = if kind == "shapes" {
            insert_shape_defaults(&new_arr[ni])
        } else {
            new_arr[ni].clone()
        };
        out.push(Edit {
            path: p,
            op: EditOp::Insert,
            value: Some(value),
        });
    }
}

/// Arrays whose elements are not fingerprinted (slides, masters, chart data)
/// are matched positionally and asserted element-for-element.
fn diff_array_positional(
    out: &mut Vec<Edit>,
    path: &[PathSegment],
    cur_arr: &[Value],
    new_arr: &[Value],
) {
    for i in 0..new_arr.len() {
        let mut p = path.to_vec();
        p.push(PathSegment::Index(i));
        if i < cur_arr.len() {
            let new_val = &new_arr[i];
            let cur_val = &cur_arr[i];
            if new_val.is_null() {
                if !cur_val.is_null() {
                    out.push(Edit {
                        path: p,
                        op: EditOp::Delete,
                        value: None,
                    });
                }
            } else if cur_val.is_object() && new_val.is_object() {
                diff_rec(out, &p, cur_val, new_val);
            } else if cur_val.is_array() && new_val.is_array() {
                diff_array(out, &p, cur_val, new_val);
            } else if cur_val != new_val {
                out.push(Edit {
                    path: p,
                    op: EditOp::Set,
                    value: Some(new_val.clone()),
                });
            }
        } else if !new_arr[i].is_null() {
            out.push(Edit {
                path: p,
                op: EditOp::Insert,
                value: Some(new_arr[i].clone()),
            });
        }
    }
    for i in new_arr.len()..cur_arr.len() {
        let mut p = path.to_vec();
        p.push(PathSegment::Index(i));
        out.push(Edit {
            path: p,
            op: EditOp::Delete,
            value: None,
        });
    }
}

/// The kind of array at `path`, from the last field segment.
fn array_kind(path: &[PathSegment]) -> Option<&'static str> {
    path.iter().rev().find_map(|seg| match seg {
        PathSegment::Field(f) => match f.as_str() {
            "slides" => Some("slides"),
            "shapes" => Some("shapes"),
            "paragraphs" => Some("paragraphs"),
            "runs" => Some("runs"),
            "grid" => Some("grid"),
            _ => None,
        },
        PathSegment::Index(_) => None,
    })
}

/// A stable identity fingerprint used to match array elements across
/// documents. The fingerprint must not change when the element is edited in
/// place, so that untouched siblings stay matched when an insertion or removal
/// shifts them:
/// - slides identify by the ordered signature of their shapes (`type|name`),
///   so text and style edits inside a slide stay matched;
/// - shapes identify by type and name (geometry and text edits stay matched);
/// - paragraphs identify by style (text edits stay matched);
/// - runs identify by formatting (text edits stay matched);
/// - grid columns identify by their width;
/// - table rows and cells are positional: they have no stable identity, and a
///   positional match keeps in-place text edits on the nested run (lossless)
///   instead of rebuilding the row or cell from defaults.
fn fingerprint(kind: &str, v: &Value) -> String {
    match kind {
        "slides" => {
            let Some(obj) = v.as_object() else {
                return String::new();
            };
            let Some(shapes) = obj.get("shapes").and_then(Value::as_array) else {
                return String::new();
            };
            let sig: Vec<String> = shapes
                .iter()
                .map(|s| {
                    let ty = s.get("shape_type").and_then(Value::as_str).unwrap_or("");
                    let name = s.get("name").and_then(Value::as_str).unwrap_or("");
                    format!("{ty}|{name}")
                })
                .collect();
            sig.join(";")
        }
        "shapes" => {
            let Some(obj) = v.as_object() else {
                return String::new();
            };
            let ty = obj.get("shape_type").and_then(Value::as_str).unwrap_or("");
            let name = obj.get("name").and_then(Value::as_str).unwrap_or("");
            format!("{ty}|{name}")
        }
        "grid" => v
            .get("width")
            .and_then(Value::as_i64)
            .map(|w| w.to_string())
            .unwrap_or_default(),
        "runs" => {
            let Some(font) = v.get("font").and_then(Value::as_object) else {
                return String::new();
            };
            let mut parts: Vec<String> = font
                .iter()
                .filter(|(k, _)| k.as_str() != "text")
                .map(|(k, val)| {
                    let s = match val {
                        Value::Bool(b) => b.to_string(),
                        Value::Number(n) => n.to_string(),
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    format!("{k}={s}")
                })
                .collect();
            parts.sort();
            parts.join("|")
        }
        "paragraphs" => {
            let Some(obj) = v.as_object() else {
                return String::new();
            };
            let mut parts = Vec::new();
            for (key, val) in obj {
                if key == "runs" {
                    continue;
                }
                let s = match val {
                    Value::Bool(b) => b.to_string(),
                    Value::Number(n) => n.to_string(),
                    Value::String(s) => s.clone(),
                    Value::Object(map) => {
                        let mut inner: Vec<String> = map
                            .iter()
                            .map(|(k, vv)| format!("{k}={}", scalar_str(vv)))
                            .collect();
                        inner.sort();
                        inner.join(",")
                    }
                    other => other.to_string(),
                };
                parts.push(format!("{key}={s}"));
            }
            parts.sort();
            parts.join("|")
        }
        _ => unreachable!("array_kind only yields shapes, paragraphs, runs, grid"),
    }
}

fn scalar_str(v: &Value) -> String {
    match v {
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Longest-common-subsequence alignment of two fingerprint sequences. Returns
/// the matched index pairs (in document order) plus the unmatched indices on
/// each side, which become the recursive diffs, deletes and inserts.
fn match_sequences(
    cur: &[String],
    new: &[String],
) -> (Vec<(usize, usize)>, Vec<usize>, Vec<usize>) {
    let n = cur.len();
    let m = new.len();
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if cur[i] == new[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut pairs = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if cur[i] == new[j] {
            pairs.push((i, j));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    let mut matched_cur = vec![false; n];
    let mut matched_new = vec![false; m];
    for &(ci, ni) in &pairs {
        matched_cur[ci] = true;
        matched_new[ni] = true;
    }
    let unmatched_cur: Vec<usize> = (0..n).filter(|&k| !matched_cur[k]).collect();
    let unmatched_new: Vec<usize> = (0..m).filter(|&k| !matched_new[k]).collect();
    (pairs, unmatched_cur, unmatched_new)
}

/// A new shape must deserialize as a `ShapeDto`: default the fields the
/// markdown mirror deliberately omits. `shape_id` 0 means "auto-assign".
pub fn insert_shape_defaults(v: &Value) -> Value {
    let Value::Object(map) = v else {
        return v.clone();
    };
    let mut m = map.clone();
    let has_tf = m.contains_key("text_frame");
    m.entry("shape_id".to_string()).or_insert(Value::from(0));
    m.entry("is_placeholder".to_string())
        .or_insert(Value::from(false));
    m.entry("has_text_frame".to_string())
        .or_insert(Value::from(has_tf));
    Value::Object(m)
}

/// Order edits so array indices stay valid during application.
///
/// Sets are applied first (they address original array positions). Deletes
/// next, highest index first, so removing a low index cannot shift an element
/// whose edit was already applied. Element inserts next (they append into the
/// arrays of their parents, which still sit at their original positions).
///
/// Slide-list edits run last: they are the only edits that reshape the slide
/// array itself, so every edit addressing a slide by its original index must
/// complete first. Slide-list deletes then run highest-index-first (they
/// address original positions in the untouched array), and slide-list inserts
/// ascending by target index (each new-document index stays valid as the
/// array grows).
pub fn order_edits(edits: Vec<Edit>) -> Vec<Edit> {
    let mut deletes: Vec<Edit> = Vec::new();
    let mut sets: Vec<Edit> = Vec::new();
    let mut element_inserts: Vec<Edit> = Vec::new();
    let mut slide_deletes: Vec<Edit> = Vec::new();
    let mut slide_inserts: Vec<Edit> = Vec::new();

    for edit in edits {
        match edit.op {
            EditOp::Set | EditOp::Replace => sets.push(edit),
            EditOp::Delete => {
                if is_slide_list_edit(&edit) {
                    slide_deletes.push(edit);
                } else {
                    deletes.push(edit);
                }
            }
            EditOp::Insert => {
                if is_slide_list_edit(&edit) {
                    slide_inserts.push(edit);
                } else {
                    element_inserts.push(edit);
                }
            }
        }
    }

    deletes.sort_by_key(|a| std::cmp::Reverse(terminal_index(a)));
    element_inserts.sort_by_key(terminal_index);
    slide_deletes.sort_by_key(|a| std::cmp::Reverse(terminal_index(a)));
    slide_inserts.sort_by_key(terminal_index);

    sets.append(&mut deletes);
    sets.append(&mut element_inserts);
    sets.append(&mut slide_deletes);
    sets.append(&mut slide_inserts);
    sets
}

/// Whether the edit targets the slide list itself (`slides[i]`) rather than a
/// slide's contents. Only slide-list edits change the slide array's shape.
fn is_slide_list_edit(edit: &Edit) -> bool {
    matches!(
        edit.path.as_slice(),
        [PathSegment::Field(f), PathSegment::Index(_)] if f == "slides"
    )
}

/// The array index addressed by the final `Index` segment of the path, or
/// `usize::MAX` when the edit does not target an array element (object-field
/// deletes and sets, which are not order-sensitive).
fn terminal_index(edit: &Edit) -> usize {
    edit.path
        .iter()
        .rev()
        .find_map(|seg| match seg {
            PathSegment::Index(i) => Some(*i),
            PathSegment::Field(_) => None,
        })
        .unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn field(name: &str) -> PathSegment {
        PathSegment::Field(name.to_string())
    }

    fn index(i: usize) -> PathSegment {
        PathSegment::Index(i)
    }

    #[test]
    fn identical_documents_produce_no_edits() {
        let v = json!({"a": {"b": 1}, "c": [1, 2, {"x": "y"}]});
        assert!(diff(&v, &v).is_empty());
    }

    #[test]
    fn scalar_change_is_a_set() {
        let edits = diff(&json!({"left": 1}), &json!({"left": 2}));
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].path, vec![field("left")]);
        assert_eq!(edits[0].op, EditOp::Set);
        assert_eq!(edits[0].value, Some(json!(2)));
    }

    #[test]
    fn absent_field_is_deleted() {
        let edits = diff(&json!({"a": 1, "b": 2}), &json!({"a": 1}));
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].op, EditOp::Delete);
        assert_eq!(edits[0].path, vec![field("b")]);
    }

    #[test]
    fn removed_nested_key_deletes() {
        let edits = diff(
            &json!({"fill": {"type": "solid", "color": {"rgb": "000000"}}}),
            &json!({"fill": {"type": "solid"}}),
        );
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].op, EditOp::Delete);
        assert_eq!(edits[0].path, vec![field("fill"), field("color")]);
    }

    #[test]
    fn new_key_is_added() {
        let edits = diff(&json!({"a": 1}), &json!({"a": 1, "b": 2}));
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].op, EditOp::Set);
        assert_eq!(edits[0].path, vec![field("b")]);
        assert_eq!(edits[0].value, Some(json!(2)));
    }

    #[test]
    fn null_deletes_field() {
        let edits = diff(&json!({"fill": {"type": "solid"}}), &json!({"fill": null}));
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].op, EditOp::Delete);
        assert_eq!(edits[0].path, vec![field("fill")]);
    }

    #[test]
    fn object_edit_lands_on_leaf() {
        let cur = json!({"fill": {"type": "solid", "color": {"rgb": "000000"}}});
        let new = json!({"fill": {"type": "solid", "color": {"rgb": "FF0000"}}});
        let edits = diff(&cur, &new);
        assert_eq!(edits.len(), 1);
        assert_eq!(
            edits[0].path,
            vec![field("fill"), field("color"), field("rgb")]
        );
        assert_eq!(edits[0].value, Some(json!("FF0000")));
    }

    #[test]
    fn shortened_array_deletes_tail() {
        let edits = diff(&json!([1, 2, 3]), &json!([1, 2]));
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].op, EditOp::Delete);
        assert_eq!(edits[0].path, vec![index(2)]);
    }

    #[test]
    fn grown_array_inserts_tail() {
        let edits = diff(&json!([1, 2]), &json!([1, 2, 3]));
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].op, EditOp::Insert);
        assert_eq!(edits[0].path, vec![index(2)]);
        assert_eq!(edits[0].value, Some(json!(3)));
    }

    #[test]
    fn array_element_edit_recurses() {
        let cur = json!({"paragraphs": [{"runs": [{"text": "a"}]}]});
        let new = json!({"paragraphs": [{"runs": [{"text": "b"}]}]});
        let edits = diff(&cur, &new);
        assert_eq!(edits.len(), 1);
        assert_eq!(
            edits[0].path,
            vec![
                field("paragraphs"),
                index(0),
                field("runs"),
                index(0),
                field("text")
            ]
        );
        assert_eq!(edits[0].value, Some(json!("b")));
    }

    #[test]
    fn deletes_sort_before_sets() {
        let edits = vec![
            Edit {
                path: vec![field("shapes"), index(0), field("left")],
                op: EditOp::Set,
                value: Some(json!(1)),
            },
            Edit {
                path: vec![field("shapes"), index(3)],
                op: EditOp::Delete,
                value: None,
            },
            Edit {
                path: vec![field("shapes"), index(2)],
                op: EditOp::Delete,
                value: None,
            },
        ];
        let ordered = order_edits(edits);
        let ops: Vec<EditOp> = ordered.iter().map(|e| e.op).collect();
        assert_eq!(ops, vec![EditOp::Set, EditOp::Delete, EditOp::Delete]);
        assert_eq!(ordered[1].path, vec![field("shapes"), index(3)]);
        assert_eq!(ordered[2].path, vec![field("shapes"), index(2)]);
    }

    #[test]
    fn null_array_element_deletes_in_place() {
        let cur = json!([{"a": 1}, {"b": 2}, {"c": 3}]);
        let new = json!([null, {"b": 5}, {"c": 3}]);
        let edits = diff(&cur, &new);
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0].op, EditOp::Delete);
        assert_eq!(edits[0].path, vec![index(0)]);
        assert_eq!(edits[1].op, EditOp::Set);
        assert_eq!(edits[1].path, vec![index(1), field("b")]);
        assert_eq!(edits[1].value, Some(json!(5)));
        // The delete at index 0 must not reorder the edit applied at index 1.
        let ordered = order_edits(edits);
        assert_eq!(ordered[0].op, EditOp::Set);
        assert_eq!(ordered[0].path, vec![index(1), field("b")]);
        assert_eq!(ordered[1].op, EditOp::Delete);
        assert_eq!(ordered[1].path, vec![index(0)]);
    }

    fn slide(name: &str) -> Value {
        json!({ "shapes": [{"shape_type": "TEXT_BOX", "name": name}] })
    }

    #[test]
    fn slide_shape_edit_is_a_replace() {
        // A slide that gains a shape at its original index is regenerated in
        // place rather than deleted and reinserted.
        let cur = json!({ "slides": [slide("A"), slide("B"), slide("C")] });
        let new = json!({
            "slides": [
                slide("A"),
                { "shapes": [{"shape_type": "TEXT_BOX", "name": "B"}, {"shape_type": "TEXT_BOX", "name": "New"}] },
                slide("C")
            ]
        });
        let edits = diff(&cur, &new);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].op, EditOp::Replace);
        assert_eq!(edits[0].path, vec![field("slides"), index(1)]);
    }

    #[test]
    fn slide_insert_in_middle_is_an_insert() {
        let cur = json!({ "slides": [slide("A"), slide("B")] });
        let new = json!({ "slides": [slide("A"), slide("X"), slide("B")] });
        let edits = diff(&cur, &new);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].op, EditOp::Insert);
        assert_eq!(edits[0].path, vec![field("slides"), index(1)]);
    }

    #[test]
    fn slide_removal_is_a_delete() {
        let cur = json!({ "slides": [slide("A"), slide("B"), slide("C")] });
        let new = json!({ "slides": [slide("A"), slide("C")] });
        let edits = diff(&cur, &new);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].op, EditOp::Delete);
        assert_eq!(edits[0].path, vec![field("slides"), index(1)]);
    }

    #[test]
    fn slide_reorder_is_delete_plus_insert() {
        let cur = json!({ "slides": [slide("A"), slide("B"), slide("C")] });
        let new = json!({ "slides": [slide("C"), slide("A"), slide("B")] });
        let edits = diff(&cur, &new);
        let mut ops: Vec<EditOp> = edits.iter().map(|e| e.op).collect();
        ops.sort_by_key(|o| match o {
            EditOp::Delete => 0,
            _ => 1,
        });
        assert_eq!(ops, vec![EditOp::Delete, EditOp::Insert]);
        assert_eq!(edits[0].path, vec![field("slides"), index(2)]);
        assert_eq!(edits[1].path, vec![field("slides"), index(0)]);
    }

    #[test]
    fn slide_replace_and_insert_ordering() {
        // A slide edited in place (shape added, so its signature changed) is a
        // Replace and must apply before a slide-list insert, which reshapes
        // the slide array.
        let cur = json!({
            "slides": [
                { "shapes": [{"shape_type": "TEXT_BOX", "name": "A"}] },
                { "shapes": [{"shape_type": "TEXT_BOX", "name": "B"}] }
            ]
        });
        let new = json!({
            "slides": [
                { "shapes": [{"shape_type": "TEXT_BOX", "name": "A"}, {"shape_type": "TEXT_BOX", "name": "X"}] },
                { "shapes": [{"shape_type": "TEXT_BOX", "name": "Z"}] },
                { "shapes": [{"shape_type": "TEXT_BOX", "name": "B"}] }
            ]
        });
        let edits = diff(&cur, &new);
        assert_eq!(edits.len(), 2);
        let ordered = order_edits(edits);
        assert_eq!(ordered[0].op, EditOp::Replace);
        assert_eq!(ordered[0].path, vec![field("slides"), index(0)]);
        assert_eq!(ordered[1].op, EditOp::Insert);
        assert_eq!(ordered[1].path, vec![field("slides"), index(1)]);
    }
}
