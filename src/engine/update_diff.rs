use serde_json::Value;

use crate::path::PathSegment;

/// Fields that are replaced as a whole rather than diffed field-by-field. The
/// DTO models them as a single document so it makes no sense to patch their
/// inner leaves.
const ATOMIC_FIELDS: [&str; 1] = ["default_paragraph_style"];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EditOp {
    /// Replace the node at `path` with `value`.
    Set,
    /// Remove the node at `path` (element, property, or attribute).
    Delete,
    /// Append `value` to the array at `path` (only ever produced for an index
    /// one past the current end, so it is always an append).
    Insert,
}

#[derive(Debug, Clone)]
pub struct Edit {
    pub path: Vec<PathSegment>,
    pub op: EditOp,
    pub value: Option<Value>,
}

/// Deeply compare the current JSON projection of the deck against the edited
/// overlay and emit the minimal set of edits needed to make the deck match.
///
/// Semantics:
/// - Objects are full-state snapshots: a key present in `current` but absent
///   from `new` is deleted, a key absent from `current` but present in `new`
///   is added, and `null` also deletes.
/// - Arrays are positional full assertions: only the elements actually present
///   in `new` are considered, an element absent from `new` (beyond its length)
///   is deleted, and elements only in `new` are inserted (appended).
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
                    None => out.push(Edit {
                        path: p,
                        op: EditOp::Delete,
                        value: None,
                    }),
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
            } else if cur_val.is_null() {
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

/// Order edits so array indices stay valid during application.
///
/// Sets are applied first (they address original array positions). Deletes
/// next, highest index first, so removing a low index cannot shift an element
/// whose edit was already applied. Inserts last, since they always append.
pub fn order_edits(edits: Vec<Edit>) -> Vec<Edit> {
    let mut deletes: Vec<Edit> = edits
        .iter()
        .filter(|e| e.op == EditOp::Delete)
        .cloned()
        .collect();
    let mut sets: Vec<Edit> = edits
        .iter()
        .filter(|e| e.op == EditOp::Set)
        .cloned()
        .collect();
    let mut inserts: Vec<Edit> = edits
        .iter()
        .filter(|e| e.op == EditOp::Insert)
        .cloned()
        .collect();

    deletes.sort_by_key(|a| std::cmp::Reverse(terminal_index(a)));
    inserts.sort_by_key(terminal_index);

    sets.append(&mut deletes);
    sets.append(&mut inserts);
    sets
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
}
