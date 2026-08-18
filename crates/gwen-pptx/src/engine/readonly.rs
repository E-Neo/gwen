use serde_json::{Map, Value};

use crate::path::PathSegment;
use PathSegment::{Field, Index};

/// Strip every read-only field from a value so it matches what the markdown
/// mirror can express. The `build` diff runs against this projection, and the
/// round-trip property test compares against it.
pub fn project(v: &Value) -> Value {
    fn walk(v: &Value, path: &mut Vec<PathSegment>) -> Value {
        match v {
            Value::Object(map) => {
                let mut out = Map::new();
                for (k, val) in map {
                    if skip_key(path, k) {
                        continue;
                    }
                    path.push(PathSegment::Field(k.clone()));
                    let proj = walk(val, path);
                    path.pop();
                    out.insert(k.clone(), proj);
                }
                Value::Object(out)
            }
            Value::Array(arr) => {
                let mut out = Vec::with_capacity(arr.len());
                for (i, val) in arr.iter().enumerate() {
                    path.push(PathSegment::Index(i));
                    out.push(walk(val, path));
                    path.pop();
                }
                Value::Array(out)
            }
            other => other.clone(),
        }
    }
    walk(v, &mut Vec::new())
}

/// Shape fields the markdown mirror cannot express. The projection strips them
/// and the apply engine rejects edits to them; `shape_type` is the exception —
/// the mirror expresses it as the `type` token, so it stays in the projection,
/// but the engine still refuses to change it because it fixes the shape's
/// identity.
pub const SHAPE_PROJECTION_FIELDS: &[&str] = &[
    "shape_id",
    "is_placeholder",
    "has_text_frame",
    "placeholder_format",
    "image",
    "ch_off_x",
    "ch_off_y",
    "ch_ext_cx",
    "ch_ext_cy",
    "shapes",
    "chart",
];

/// Whether a shape field must be rejected as read-only by the apply engine.
pub fn is_read_only_shape_field(name: &str) -> bool {
    name == "shape_type" || SHAPE_PROJECTION_FIELDS.contains(&name)
}

/// Whether a field, at the given JSON path, is read-only: it is not part of
/// the editable markdown mirror, is never emitted by `markdown`, and must not
/// produce edits when absent from an edited document.
///
/// The markdown round-trip property test strips exactly these fields from the
/// snapshot before comparing against `parse(serialize(snapshot))`.
pub fn skip_key(path: &[PathSegment], key: &str) -> bool {
    match last_two(path) {
        (Some(Field(f)), Some(Index(_))) if f == "shapes" => SHAPE_PROJECTION_FIELDS.contains(&key),
        (Some(Field(f)), Some(Index(_))) if f == "slides" => key == "slide_layout",
        (Some(Field(f)), Some(Index(_))) if f == "slide_masters" => {
            matches!(key, "name" | "slide_layouts")
        }
        (Some(Field(f)), Some(Index(_))) if f == "runs" => key == "hyperlink",
        (Some(Field(f)), Some(Index(_))) if f == "rows" => key == "height",
        (Some(Field(f)), Some(Index(_))) if f == "cells" => {
            matches!(key, "row_span" | "grid_span" | "h_merge" | "v_merge")
        }
        _ => cell_text_frame_field(path, key) || cell_paragraph_field(path, key),
    }
}

/// A cell's `text_frame` carries only its paragraphs in the mirror; the
/// frame-level properties and default style are preserved from the original.
fn cell_text_frame_field(path: &[PathSegment], key: &str) -> bool {
    let n = path.len();
    let is_cell_tf = n >= 3
        && matches!(&path[n - 1], Field(f) if f == "text_frame")
        && matches!(&path[n - 2], Index(_))
        && matches!(&path[n - 3], Field(f) if f == "cells");
    is_cell_tf
        && matches!(
            key,
            "auto_size"
                | "word_wrap"
                | "vertical_anchor"
                | "margin_left"
                | "margin_right"
                | "margin_top"
                | "margin_bottom"
                | "default_paragraph_style"
        )
}

/// Cell paragraphs carry no style in the mirror; alignment, level, spacing and
/// font defaults are preserved from the original deck.
fn cell_paragraph_field(path: &[PathSegment], key: &str) -> bool {
    let n = path.len();
    let is_cell_para = n >= 5
        && matches!(&path[n - 1], Index(_))
        && matches!(&path[n - 2], Field(f) if f == "paragraphs")
        && matches!(&path[n - 3], Field(f) if f == "text_frame")
        && matches!(&path[n - 4], Index(_))
        && matches!(&path[n - 5], Field(f) if f == "cells");
    is_cell_para
        && matches!(
            key,
            "alignment" | "level" | "line_spacing" | "space_before" | "space_after" | "font"
        )
}

fn last_two(path: &[PathSegment]) -> (Option<&PathSegment>, Option<&PathSegment>) {
    let n = path.len();
    (path.get(n.wrapping_sub(2)), path.get(n.wrapping_sub(1)))
}
