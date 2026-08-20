use std::collections::HashMap;
use std::path::Path;

use serde_json::{Map, Value};

use super::error::MdResult;
use super::markers::{
    ATTR_CH_EXT_CX, ATTR_CH_EXT_CY, ATTR_CH_OFF_X, ATTR_CH_OFF_Y, ATTR_CHART_TYPE, ATTR_GRID,
    ATTR_ID, ATTR_IMAGE, ATTR_MERGE, ATTR_NAME, ATTR_PH_IDX, ATTR_PH_SZ, ATTR_PH_TYPE,
    ATTR_PLACEHOLDER, ATTR_ROTATION, ATTR_ROW_HEIGHTS, ATTR_SRC, ATTR_TYPE, ATTR_WIDTH,
    LENGTH_ATTRS, MARKER_BACKGROUND, MARKER_END_GROUP, MARKER_LAYOUT, MARKER_MASTER, MARKER_SLIDE,
    legend, shape_type_token,
};
use super::normalize;
use super::style::{
    Decl, StyleRegistry, para_decls, quote, run_decls, scalar, shape_decls, shape_kind, tf_decls,
};

/// Serialize a presentation snapshot (the JSON produced by `query_value`) into
/// the markdown mirror. The output is a faithful, editable representation of
/// every field the apply engine can change; read-only fields (shape ids,
/// placeholder flags, slide layout references, chart data, hyperlinks, ...)
/// are deliberately omitted and reconstructed from the original deck on apply.
///
/// Format (see `docs/mirror-format.md` and `markers::legend`):
/// - YAML front matter (`pptx`, `core_properties`, `theme`) delimited by `---`,
///   followed by the always-on legend comment;
/// - a `<style>` block of deduplicated, content-addressed CSS classes. Each
///   shape class folds the shape's fill/outline, its frame properties and its
///   default paragraph style into one rule, referenced by the shape marker's
///   `class=` attribute;
/// - shape identity and geometry live in the shape marker's HTML attributes
///   (`<!-- shape type="textbox" class="textbox-1" name="TextBox 4"
///   left="914400" ... -->`);
/// - `# Master N` sections, `## Slide N` headings (structural anchors whose
///   text is ignored), `### Notes` sections, and body paragraphs rendered from
///   runs with native emphasis where possible. A `<!-- paragraph class="..."
///   -->` marker precedes any paragraph whose own style deviates from the
///   shape default.
pub fn serialize(snapshot: &Value) -> String {
    let doc = normalize::normalize(snapshot);
    let obj = doc.as_object().expect("snapshot is an object");

    let mut reg = StyleRegistry::new();
    collect_classes(&mut reg, &doc);

    let mut out = String::new();
    write_front_matter(&mut out, obj);
    out.push('\n');
    out.push_str(legend());
    out.push('\n');
    out.push_str(&reg.to_style_block());
    out.push('\n');

    if let Some(masters) = obj.get("slide_masters").and_then(Value::as_array) {
        for (i, master) in masters.iter().enumerate() {
            out.push_str(&format!("# Master {}\n\n", i + 1));
            if let Some(shapes) = master.get("shapes").and_then(Value::as_array) {
                for shape in shapes {
                    write_shape(&mut out, &mut reg, shape, false);
                }
            }
            out.push('\n');
        }
    }

    if let Some(slides) = obj.get("slides").and_then(Value::as_array) {
        for (i, slide) in slides.iter().enumerate() {
            write_slide(&mut out, &mut reg, slide, i);
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Pass 1: register every class the emission pass will reference
// ---------------------------------------------------------------------------

fn collect_classes(reg: &mut StyleRegistry, doc: &Value) {
    let obj = doc.as_object().expect("snapshot is an object");
    for list in ["slide_masters", "slides"] {
        let Some(elems) = obj.get(list).and_then(Value::as_array) else {
            continue;
        };
        for el in elems {
            if let Some(shapes) = el.get("shapes").and_then(Value::as_array) {
                for shape in shapes {
                    register_shape(reg, shape);
                }
            }
            if list == "slides"
                && let Some(notes) = el.get("notes").filter(|v| !v.is_null())
                && let Some(shapes) = notes.get("shapes").and_then(Value::as_array)
            {
                for shape in shapes {
                    register_shape(reg, shape);
                }
            }
        }
    }
}

fn register_shape(reg: &mut StyleRegistry, shape: &Value) {
    let Some(obj) = shape.as_object() else {
        return;
    };
    let shape_type = obj.get("shape_type").and_then(Value::as_str).unwrap_or("");
    let auto = obj.get("auto_shape_type").and_then(Value::as_str);
    reg.class_for(&shape_kind(shape_type, auto), &shape_class_decls(obj));
    if let Some(tf) = obj.get("text_frame").and_then(Value::as_object) {
        if let Some(paras) = tf.get("paragraphs").and_then(Value::as_array) {
            for para in paras {
                register_para(reg, para);
            }
        }
    } else if let Some(table) = obj.get("table").and_then(Value::as_object)
        && let Some(rows) = table.get("rows").and_then(Value::as_array)
    {
        for row in rows {
            if let Some(cells) = row.get("cells").and_then(Value::as_array) {
                for cell in cells {
                    register_cell(reg, cell);
                }
            }
        }
    }
    if let Some(children) = obj.get("shapes").and_then(Value::as_array) {
        for child in children {
            register_shape(reg, child);
        }
    }
}

/// The declarations for a shape's styling class: the shape fill/outline plus,
/// for text frames, the frame body properties and the default paragraph style
/// folded into the same rule. On parse one class feeds `shape_from_decls`,
/// `tf_from_decls` and `para_from_decls`.
fn shape_class_decls(obj: &Map<String, Value>) -> Vec<Decl> {
    let mut decls = shape_decls(obj);
    if let Some(tf) = obj.get("text_frame").and_then(Value::as_object) {
        decls.extend(tf_decls(tf));
        if let Some(dps) = tf.get("default_paragraph_style").and_then(Value::as_object) {
            decls.extend(para_decls(dps));
        }
    }
    decls
}

fn register_para(reg: &mut StyleRegistry, para: &Value) {
    let Some(obj) = para.as_object() else {
        return;
    };
    reg.class_for("para", &para_decls(obj));
    register_runs(
        reg,
        obj.get("runs")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]),
    );
}

fn register_cell(reg: &mut StyleRegistry, cell: &Value) {
    let Some(tf) = cell.get("text_frame").and_then(Value::as_object) else {
        return;
    };
    let Some(paras) = tf.get("paragraphs").and_then(Value::as_array) else {
        return;
    };
    for para in paras {
        register_runs(
            reg,
            para.get("runs")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or(&[]),
        );
    }
}

fn register_runs(reg: &mut StyleRegistry, runs: &[Value]) {
    for run in runs {
        let Some(font) = run
            .get("font")
            .and_then(Value::as_object)
            .filter(|f| !f.is_empty())
        else {
            continue;
        };
        if native_emphasis(font).is_none() {
            reg.class_for("run", &run_decls(font));
        }
    }
}

// ---------------------------------------------------------------------------
// Pass 2: emission
// ---------------------------------------------------------------------------

fn write_slide(out: &mut String, reg: &mut StyleRegistry, slide: &Value, idx: usize) {
    let Some(obj) = slide.as_object() else {
        return;
    };
    let shapes = obj.get("shapes").and_then(Value::as_array);
    let Some(shapes) = shapes else {
        return;
    };
    out.push_str(&format!("## Slide {}\n", idx + 1));
    write_background(out, slide);
    out.push('\n');

    for shape in shapes {
        write_shape(out, reg, shape, false);
    }

    if let Some(notes) = obj.get("notes").filter(|v| !v.is_null()) {
        out.push_str("### Notes\n\n");
        if let Some(notes_shapes) = notes.get("shapes").and_then(Value::as_array) {
            for shape in notes_shapes {
                write_shape(out, reg, shape, false);
            }
        }
        out.push('\n');
    }
    out.push('\n');
}

fn write_background(out: &mut String, slide: &Value) {
    let Some(fill) = slide
        .get("background")
        .and_then(Value::as_object)
        .and_then(|b| b.get("fill"))
    else {
        return;
    };
    let ty = fill.get("type").and_then(Value::as_str).unwrap_or("");
    if ty.is_empty() {
        return;
    }
    let color = fill.get("color").and_then(Value::as_str).unwrap_or("");
    let val = if color.is_empty() {
        ty.to_ascii_uppercase()
    } else {
        format!("{}:{}", ty.to_ascii_uppercase(), color)
    };
    out.push_str(&format!("<!-- {MARKER_BACKGROUND} fill=\"{}\" -->\n", val));
}

fn write_shape(out: &mut String, reg: &mut StyleRegistry, shape: &Value, complete: bool) {
    let Some(obj) = shape.as_object() else {
        return;
    };

    write_shape_marker(out, reg, obj, complete);

    let shape_type = obj.get("shape_type").and_then(Value::as_str).unwrap_or("");
    if shape_type == "GROUP" {
        if complete {
            if let Some(children) = obj.get("shapes").and_then(Value::as_array) {
                for child in children {
                    write_shape(out, reg, child, true);
                }
            }
            out.push_str(&format!("<!-- {MARKER_END_GROUP} -->\n"));
        }
    } else if shape_type == "CHART" && complete {
        write_chart_table(out, obj);
    } else if let Some(tf) = obj.get("text_frame").and_then(Value::as_object) {
        if let Some(paras) = tf.get("paragraphs").and_then(Value::as_array) {
            for para in paras {
                write_paragraph(out, reg, para, complete);
            }
        }
    } else if let Some(table) = obj.get("table") {
        write_table(out, reg, table, complete);
    }
}

/// Emit the shape marker: `<!-- shape type="textbox" class="textbox-1"
/// id="4" name="TextBox 4" left="914400" top="152400" width="6096000"
/// height="914400" rotation="-20" placeholder ph-type="TITLE" ph-idx="1"
/// ph-sz="full" image="image1.png" grid="775018,936978" row-heights="1000"
/// merge="1,0,2,1,0,0" crop-left="0.1" ch-off-x="0" ch-off-y="0" ch-ext-cx="0"
/// ch-ext-cy="0" -->`. Attributes are emitted only for non-null snapshot values.
fn write_shape_marker(
    out: &mut String,
    reg: &mut StyleRegistry,
    obj: &Map<String, Value>,
    complete: bool,
) {
    let shape_type = obj.get("shape_type").and_then(Value::as_str).unwrap_or("");
    let auto = obj.get("auto_shape_type").and_then(Value::as_str);
    let sclass = reg.class_for(&shape_kind(shape_type, auto), &shape_class_decls(obj));

    let mut parts = vec![format!("{ATTR_TYPE}=\"{}\"", shape_type_token(shape_type))];
    if let Some(v) = auto {
        parts.push(format!("auto-shape=\"{}\"", escape_attr(v)));
    }
    parts.push(format!("class=\"{sclass}\""));
    if complete && let Some(v) = obj.get("shape_id") {
        parts.push(format!("{ATTR_ID}=\"{}\"", scalar(v)));
    }
    if let Some(v) = obj.get(ATTR_NAME).and_then(Value::as_str) {
        parts.push(format!("{ATTR_NAME}=\"{}\"", escape_attr(v)));
    }
    for key in LENGTH_ATTRS {
        if let Some(v) = obj.get(key) {
            parts.push(format!("{key}=\"{}\"", scalar(v)));
        }
    }
    if let Some(v) = obj.get(ATTR_ROTATION) {
        parts.push(format!("{ATTR_ROTATION}=\"{}\"", scalar(v)));
    }
    if complete {
        if obj.get("is_placeholder").and_then(Value::as_bool) == Some(true) {
            parts.push(ATTR_PLACEHOLDER.to_string());
        }
        if let Some(pf) = obj.get("placeholder_format").and_then(Value::as_object) {
            if let Some(v) = pf.get("type").and_then(Value::as_str) {
                parts.push(format!("{ATTR_PH_TYPE}=\"{}\"", escape_attr(v)));
            }
            if let Some(v) = pf.get("idx") {
                parts.push(format!("{ATTR_PH_IDX}=\"{}\"", scalar(v)));
            }
            if let Some(v) = pf.get("sz").and_then(Value::as_str) {
                parts.push(format!("{ATTR_PH_SZ}=\"{}\"", escape_attr(v)));
            }
        }
        if let Some(v) = obj.get(ATTR_IMAGE).and_then(Value::as_str) {
            parts.push(format!("{ATTR_IMAGE}=\"{}\"", escape_attr(v)));
        }
        for (key, attr) in [
            ("ch_off_x", ATTR_CH_OFF_X),
            ("ch_off_y", ATTR_CH_OFF_Y),
            ("ch_ext_cx", ATTR_CH_EXT_CX),
            ("ch_ext_cy", ATTR_CH_EXT_CY),
        ] {
            if let Some(v) = obj.get(key) {
                parts.push(format!("{attr}=\"{}\"", scalar(v)));
            }
        }
        if let Some(chart_type) = obj
            .get("chart")
            .and_then(Value::as_object)
            .and_then(|c| c.get("chart_type"))
            .and_then(Value::as_str)
        {
            parts.push(format!("{ATTR_CHART_TYPE}=\"{}\"", escape_attr(chart_type)));
        }
    }
    if let Some(table) = obj.get("table").and_then(Value::as_object) {
        if let Some(grid) = table.get(ATTR_GRID).and_then(Value::as_array).map(|g| {
            g.iter()
                .filter_map(|c| c.as_object().and_then(|o| o.get(ATTR_WIDTH)).map(scalar))
                .collect::<Vec<_>>()
                .join(",")
        }) {
            parts.push(format!("{ATTR_GRID}=\"{grid}\""));
        }
        if complete {
            if let Some(heights) = table
                .get("rows")
                .and_then(Value::as_array)
                .map(|rows| {
                    rows.iter()
                        .filter_map(|r| r.get("height"))
                        .map(scalar)
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .filter(|s| !s.is_empty())
            {
                parts.push(format!("{ATTR_ROW_HEIGHTS}=\"{heights}\""));
            }
            if let Some(merges) = table
                .get("rows")
                .and_then(Value::as_array)
                .map(|rows| merge_tokens(rows))
                .filter(|t| !t.is_empty())
            {
                parts.push(format!("{ATTR_MERGE}=\"{}\"", merges.join(";")));
            }
        }
    }
    if let Some(crop) = obj.get("crop").and_then(Value::as_object) {
        for side in ["left", "top", "right", "bottom"] {
            if let Some(v) = crop.get(side) {
                parts.push(format!("crop-{side}=\"{}\"", scalar(v)));
            }
        }
    }
    out.push_str(&format!("<!-- shape {} -->\n", parts.join(" ")));
}

/// Per-cell merge tokens (`r,c,gridSpan,rowSpan,hMerge,vMerge`) for every
/// table cell carrying a merge property.
fn merge_tokens(rows: &[Value]) -> Vec<String> {
    let mut out = Vec::new();
    for (ri, row) in rows.iter().enumerate() {
        let Some(cells) = row.get("cells").and_then(Value::as_array) else {
            continue;
        };
        for (ci, cell) in cells.iter().enumerate() {
            let Some(c) = cell.as_object() else {
                continue;
            };
            let grid_span = c.get("grid_span").and_then(Value::as_u64).unwrap_or(0);
            let row_span = c.get("row_span").and_then(Value::as_u64).unwrap_or(0);
            let h_merge = c.get("h_merge").and_then(Value::as_bool).unwrap_or(false);
            let v_merge = c.get("v_merge").and_then(Value::as_bool).unwrap_or(false);
            if grid_span > 0 || row_span > 0 || h_merge || v_merge {
                out.push(format!(
                    "{ri},{ci},{grid_span},{row_span},{},",
                    if h_merge { 1 } else { 0 }
                ));
                let last = out.last_mut().expect("just pushed");
                last.push(if v_merge { '1' } else { '0' });
            }
        }
    }
    out
}

/// The series table under a `type="chart"` marker: header row = categories,
/// later rows = series name + values.
fn write_chart_table(out: &mut String, obj: &Map<String, Value>) {
    let Some(chart) = obj.get("chart").and_then(Value::as_object) else {
        out.push('\n');
        return;
    };
    let Some(series) = chart.get("series").and_then(Value::as_array) else {
        out.push('\n');
        return;
    };
    let cats: Vec<String> = series
        .first()
        .and_then(|s| s.get("categories"))
        .and_then(Value::as_array)
        .map(|a| a.iter().map(chart_cell).collect())
        .unwrap_or_default();
    let cols = cats.len() + 1;
    out.push_str(&format!(
        "| {} |\n",
        std::iter::once(String::new())
            .chain(cats.clone())
            .collect::<Vec<_>>()
            .join(" | ")
    ));
    out.push_str(&format!("| {} |\n", vec!["---"; cols].join(" | ")));
    for s in series {
        let name = s.get("name").and_then(Value::as_str).unwrap_or("");
        let values: Vec<String> = s
            .get("values")
            .and_then(Value::as_array)
            .map(|a| a.iter().map(scalar).collect())
            .unwrap_or_default();
        out.push_str(&format!(
            "| {} | {} |\n",
            chart_cell_str(name),
            values.join(" | ")
        ));
    }
    out.push('\n');
}

fn chart_cell(v: &Value) -> String {
    chart_cell_str(v.as_str().unwrap_or(""))
}

fn chart_cell_str(s: &str) -> String {
    escape_text(s)
}

fn write_paragraph(out: &mut String, reg: &mut StyleRegistry, para: &Value, complete: bool) {
    let Some(obj) = para.as_object() else {
        return;
    };
    if !para_decls(obj).is_empty() {
        let pclass = reg.class_for("para", &para_decls(obj));
        out.push_str(&format!("<!-- paragraph class=\"{pclass}\" -->\n"));
    }
    let runs = obj
        .get("runs")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let content = render_runs(reg, runs, complete);
    if content.is_empty() {
        out.push_str("<span></span>\n\n");
    } else {
        out.push_str(&guard_block_markers(&content));
        out.push_str("\n\n");
    }
}

/// Escape a rendered paragraph line so pulldown-cmark never parses it as a
/// list, blockquote or fenced block.
fn guard_block_markers(s: &str) -> String {
    let risky = {
        let b = s.as_bytes();
        b.starts_with(b"- ")
            || b.starts_with(b"+ ")
            || b.starts_with(b"> ")
            || starts_with_ordered_list(b)
    };
    if risky {
        format!("\\{s}")
    } else {
        s.to_string()
    }
}

fn starts_with_ordered_list(b: &[u8]) -> bool {
    if !b.first().is_some_and(u8::is_ascii_digit) {
        return false;
    }
    let mut i = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    i < b.len() && b[i] == b'.' && (i + 1 >= b.len() || b[i + 1] == b' ' || b[i + 1] == b'\t')
}

fn render_runs(reg: &mut StyleRegistry, runs: &[Value], complete: bool) -> String {
    let mut out = String::new();
    for run in runs {
        out.push_str(&render_run(reg, run, complete));
    }
    out
}

fn render_run(reg: &mut StyleRegistry, run: &Value, complete: bool) -> String {
    let Some(obj) = run.as_object() else {
        return String::new();
    };
    let text = obj.get("text").and_then(Value::as_str).unwrap_or("");
    let link = if complete {
        obj.get("hyperlink")
            .and_then(Value::as_object)
            .and_then(|h| h.get("address"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
    } else {
        None
    };
    let Some(font) = obj
        .get("font")
        .and_then(Value::as_object)
        .filter(|f| !f.is_empty())
    else {
        let body = escape_text(text);
        return link.map_or(body.clone(), |url| format!("[{body}]({})", escape_url(url)));
    };
    let body = if let Some((open, close)) = native_emphasis(font) {
        format!("{open}{}{close}", escape_text(text))
    } else {
        let class = reg.class_for("run", &run_decls(font));
        format!("<span class=\"{class}\">{}</span>", escape_text(text))
    };
    link.map_or(body.clone(), |url| format!("[{body}]({})", escape_url(url)))
}

/// Escape a link destination so the `[text](url)` form parses back verbatim.
fn escape_url(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '(' => out.push_str("\\("),
            ')' => out.push_str("\\)"),
            _ => out.push(c),
        }
    }
    out
}

/// Emphasis markers for a font that is exactly `{bold:true}`, `{italic:true}`
/// or `{bold:true, italic:true}`. Any other key (or a false value) disqualifies
/// the run so explicit `font-weight: normal` etc. round-trips through a class.
fn native_emphasis(font: &Map<String, Value>) -> Option<(&'static str, &'static str)> {
    if font.len() > 2 {
        return None;
    }
    for (k, v) in font {
        if k != "bold" && k != "italic" {
            return None;
        }
        if !matches!(v, Value::Bool(true)) {
            return None;
        }
    }
    match (font.contains_key("bold"), font.contains_key("italic")) {
        (true, true) => Some(("***", "***")),
        (true, false) => Some(("**", "**")),
        (false, true) => Some(("*", "*")),
        (false, false) => None,
    }
}

fn write_table(out: &mut String, reg: &mut StyleRegistry, table: &Value, complete: bool) {
    let Some(obj) = table.as_object() else {
        out.push('\n');
        return;
    };
    let Some(rows) = obj.get("rows").and_then(Value::as_array) else {
        out.push('\n');
        return;
    };
    let cols = rows
        .first()
        .and_then(|r| r.get("cells").and_then(Value::as_array))
        .map(Vec::len)
        .unwrap_or(0);
    let header: Vec<String> = rows
        .first()
        .and_then(|r| r.get("cells").and_then(Value::as_array))
        .map(|cells| cells.iter().map(|c| write_cell(reg, c, complete)).collect())
        .unwrap_or_default();
    out.push_str(&format!("| {} |\n", header.join(" | ")));
    out.push_str(&format!("| {} |\n", vec!["---"; cols].join(" | ")));
    for row in rows.iter().skip(1) {
        let cells: Vec<String> = row
            .get("cells")
            .and_then(Value::as_array)
            .map(|c| c.iter().map(|c| write_cell(reg, c, complete)).collect())
            .unwrap_or_default();
        out.push_str(&format!("| {} |\n", cells.join(" | ")));
    }
    out.push('\n');
}

fn write_cell(reg: &mut StyleRegistry, cell: &Value, complete: bool) -> String {
    let Some(tf) = cell.get("text_frame").and_then(Value::as_object) else {
        return String::new();
    };
    let Some(paras) = tf.get("paragraphs").and_then(Value::as_array) else {
        return String::new();
    };
    paras
        .iter()
        .map(|p| {
            render_runs(
                reg,
                p.get("runs")
                    .and_then(Value::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]),
                complete,
            )
        })
        .collect::<Vec<_>>()
        .join("<br>")
}

// ---------------------------------------------------------------------------
// Front matter
// ---------------------------------------------------------------------------

fn write_front_matter(out: &mut String, obj: &Map<String, Value>) {
    let mut root = Map::new();
    let mut pptx = Map::new();
    pptx.insert(
        "slide_width".into(),
        obj.get("slide_width").cloned().unwrap_or(Value::from(0)),
    );
    pptx.insert(
        "slide_height".into(),
        obj.get("slide_height").cloned().unwrap_or(Value::from(0)),
    );
    root.insert("pptx".into(), Value::Object(pptx));

    if let Some(cp) = obj.get("core_properties").filter(|v| !v.is_null()) {
        root.insert("core_properties".into(), cp.clone());
    }

    let mut theme = Map::new();
    let t = obj
        .get("theme")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut colors = Map::new();
    let mut fonts = Map::new();
    if let Some(c) = t.get("colors").and_then(Value::as_object) {
        colors = c.clone();
    }
    if let Some(f) = t.get("fonts").and_then(Value::as_object) {
        fonts = f.clone();
    }
    theme.insert("colors".into(), Value::Object(colors));
    theme.insert("fonts".into(), Value::Object(fonts));
    root.insert("theme".into(), Value::Object(theme));

    out.push_str("---\n");
    write_yaml_map(out, 0, &Value::Object(root));
    out.push_str("---\n");
}

fn write_yaml_map(out: &mut String, indent: usize, v: &Value) {
    let Some(map) = v.as_object() else {
        return;
    };
    for (k, val) in map {
        write_yaml_entry(out, indent, k, val);
    }
}

/// Write `key: value` with every string double-quoted so YAML 1.1 never
/// coerces values like `yes`, `off` or dates.
fn write_yaml_entry(out: &mut String, indent: usize, key: &str, val: &Value) {
    let pad = "  ".repeat(indent);
    match val {
        Value::Object(m) if m.is_empty() => out.push_str(&format!("{pad}{key}: {{}}\n")),
        Value::Object(_) => {
            out.push_str(&format!("{pad}{key}:\n"));
            write_yaml_map(out, indent + 1, val);
        }
        Value::String(s) => out.push_str(&format!("{pad}{key}: {}\n", quote(s))),
        Value::Number(n) => out.push_str(&format!("{pad}{key}: {n}\n")),
        Value::Bool(b) => out.push_str(&format!("{pad}{key}: {b}\n")),
        Value::Null => out.push_str(&format!("{pad}{key}: null\n")),
        other => out.push_str(&format!("{pad}{key}: {}\n", quote(&other.to_string()))),
    }
}

// ---------------------------------------------------------------------------
// Text escaping
// ---------------------------------------------------------------------------

/// Escape a value for use inside a marker attribute. Backslashes and double
/// quotes are backslash-escaped, matching `unquote`.
fn escape_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            _ => out.push(c),
        }
    }
    out
}

/// Escape run text so pulldown-cmark parses it back verbatim. Characters that
/// markdown would interpret are backslash-escaped; `<`, `>`, `&` use character
/// references; newlines and edge whitespace use numeric references.
fn escape_text(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let last = chars.len().saturating_sub(1);
    let mut out = String::with_capacity(s.len());
    for (i, c) in chars.into_iter().enumerate() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\\' | '`' | '*' | '_' | '[' | ']' | '#' | '|' => {
                out.push('\\');
                out.push(c);
            }
            ' ' if i == 0 || i == last => out.push_str("&#32;"),
            _ => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Multi-file project mirror
// ---------------------------------------------------------------------------

/// Write the full project mirror for `snapshot` under `dir`:
/// `src/PRESENTATION.md` plus `src/masters/`, `src/layouts/` and
/// `src/slides/`. The snapshot must be the complete query output; the pptx
/// URIs and numeric layout indices it carries are internal and are not written
/// into the mirror — slides reference their layout by the layout's markdown
/// path instead.
pub fn write_document(snapshot: &Value, dir: &Path) -> MdResult<()> {
    let doc = normalize::normalize(snapshot);
    let obj = doc.as_object().ok_or_else(|| {
        super::error::MdError::at(
            "snapshot must be an object",
            super::error::MdSpan {
                line: 0,
                col: 0,
                offset: 0,
                len: 0,
            },
        )
    })?;
    let src = dir.join("src");
    let masters_dir = src.join("masters");
    let layouts_dir = src.join("layouts");
    let slides_dir = src.join("slides");
    for d in [&masters_dir, &layouts_dir, &slides_dir] {
        std::fs::create_dir_all(d)?;
    }

    // (master index, layout index) -> layout file path, mirroring the running
    // counter used while writing the master files below.
    let mut layout_paths: HashMap<(usize, usize), String> = HashMap::new();
    if let Some(masters) = obj.get("slide_masters").and_then(Value::as_array) {
        let mut counter = 0usize;
        for (i, master) in masters.iter().enumerate() {
            if let Some(layouts) = master.get("slide_layouts").and_then(Value::as_array) {
                for (j, _) in layouts.iter().enumerate() {
                    counter += 1;
                    layout_paths.insert((i, j), format!("layouts/layout{counter}.md"));
                }
            }
        }
    }

    let mut pres = String::new();
    write_front_matter(&mut pres, obj);
    pres.push('\n');
    pres.push_str(legend());
    pres.push('\n');

    let mut layout_file = 0usize;
    if let Some(masters) = obj.get("slide_masters").and_then(Value::as_array) {
        for (i, master) in masters.iter().enumerate() {
            let src_ref = format!("masters/master{}.md", i + 1);
            let name = master.get("name").and_then(Value::as_str).unwrap_or("");
            write_ref(
                &mut pres,
                MARKER_MASTER,
                &[("name", name), (ATTR_SRC, &src_ref)],
            );
            write_master_file(&masters_dir, &layouts_dir, &mut layout_file, i + 1, master)?;
        }
    }
    if let Some(slides) = obj.get("slides").and_then(Value::as_array) {
        for (i, slide) in slides.iter().enumerate() {
            let src_ref = format!("slides/slide{}.md", i + 1);
            let name = slide.get("name").and_then(Value::as_str).unwrap_or("");
            write_ref(
                &mut pres,
                MARKER_SLIDE,
                &[("name", name), (ATTR_SRC, &src_ref)],
            );
            write_slide_file(&slides_dir, i + 1, slide, &layout_paths)?;
        }
    }

    std::fs::write(src.join("PRESENTATION.md"), pres)?;
    Ok(())
}

/// A `<!-- key name="..." src="..." -->` reference line. Attributes
/// with empty values are omitted.
fn write_ref(out: &mut String, key: &str, attrs: &[(&str, &str)]) {
    let mut parts = Vec::new();
    for (k, v) in attrs {
        if !v.is_empty() {
            parts.push(format!("{k}=\"{}\"", escape_attr(v)));
        }
    }
    out.push_str(&format!("<!-- {key} {} -->\n", parts.join(" ")));
}

/// A per-file YAML front matter from key/value pairs, all values quoted.
fn write_file_front(out: &mut String, pairs: &[(&str, &str)]) {
    out.push_str("---\n");
    for (k, v) in pairs {
        out.push_str(&format!("{k}: {}\n", quote(v)));
    }
    out.push_str("---\n");
}

fn write_master_file(
    masters_dir: &Path,
    layouts_dir: &Path,
    layout_file: &mut usize,
    n: usize,
    master: &Value,
) -> MdResult<()> {
    let obj = master.as_object().cloned().unwrap_or_default();
    let name = obj.get("name").and_then(Value::as_str).unwrap_or("");

    let mut out = String::new();
    write_file_front(&mut out, &[("name", name)]);
    out.push('\n');
    out.push_str(&format!(
        "<!-- Gwen mirror part: masters/master{n}.md -->\n\n"
    ));

    let shapes = obj
        .get("shapes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut reg = StyleRegistry::new();
    for shape in &shapes {
        register_shape(&mut reg, shape);
    }
    out.push_str(&reg.to_style_block());
    out.push('\n');
    for shape in &shapes {
        write_shape(&mut out, &mut reg, shape, true);
    }
    out.push('\n');

    if let Some(layouts) = obj.get("slide_layouts").and_then(Value::as_array) {
        for layout in layouts {
            *layout_file += 1;
            let lsrc = format!("layouts/layout{layout_file}.md");
            let lname = layout.get("name").and_then(Value::as_str).unwrap_or("");
            write_ref(
                &mut out,
                MARKER_LAYOUT,
                &[("name", lname), (ATTR_SRC, &lsrc)],
            );
            write_layout_file(layouts_dir, *layout_file, layout)?;
        }
    }

    std::fs::write(masters_dir.join(format!("master{n}.md")), out)?;
    Ok(())
}

fn write_layout_file(layouts_dir: &Path, n: usize, layout: &Value) -> MdResult<()> {
    let obj = layout.as_object().cloned().unwrap_or_default();
    let name = obj.get("name").and_then(Value::as_str).unwrap_or("");

    let mut out = String::new();
    write_file_front(&mut out, &[("name", name)]);
    out.push('\n');
    out.push_str(&format!(
        "<!-- Gwen mirror part: layouts/layout{n}.md -->\n\n"
    ));

    let shapes = obj
        .get("shapes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut reg = StyleRegistry::new();
    for shape in &shapes {
        register_shape(&mut reg, shape);
    }
    out.push_str(&reg.to_style_block());
    out.push('\n');
    for shape in &shapes {
        write_shape(&mut out, &mut reg, shape, true);
    }
    std::fs::write(layouts_dir.join(format!("layout{n}.md")), out)?;
    Ok(())
}

fn write_slide_file(
    slides_dir: &Path,
    n: usize,
    slide: &Value,
    layout_paths: &HashMap<(usize, usize), String>,
) -> MdResult<()> {
    let obj = slide.as_object().cloned().unwrap_or_default();
    let name = obj.get("name").and_then(Value::as_str).unwrap_or("");
    let layout_path = obj
        .get("slide_layout")
        .and_then(Value::as_object)
        .and_then(|l| {
            let m = l.get("master").and_then(Value::as_i64)? as usize;
            let l = l.get("layout").and_then(Value::as_i64)? as usize;
            layout_paths.get(&(m, l)).cloned()
        });

    let mut out = String::new();
    let mut front: Vec<(&str, String)> = Vec::new();
    if !name.is_empty() {
        front.push(("name", name.to_string()));
    }
    if let Some(path) = layout_path {
        front.push(("layout", path));
    }
    write_file_front(
        &mut out,
        &front
            .iter()
            .map(|(k, v)| (*k, v.as_str()))
            .collect::<Vec<_>>(),
    );
    out.push('\n');
    out.push_str(&format!(
        "<!-- Gwen mirror part: slides/slide{n}.md -->\n\n"
    ));

    let shapes = obj
        .get("shapes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut notes_shapes: Vec<Value> = Vec::new();
    if let Some(notes) = obj.get("notes").filter(|v| !v.is_null())
        && let Some(ns) = notes.get("shapes").and_then(Value::as_array)
    {
        notes_shapes = ns.clone();
    }
    let mut reg = StyleRegistry::new();
    for shape in &shapes {
        register_shape(&mut reg, shape);
    }
    for shape in &notes_shapes {
        register_shape(&mut reg, shape);
    }
    out.push_str(&reg.to_style_block());
    out.push('\n');

    write_background(&mut out, slide);
    out.push('\n');
    for shape in &shapes {
        write_shape(&mut out, &mut reg, shape, true);
    }
    if !notes_shapes.is_empty() {
        out.push_str("### Notes\n\n");
        for shape in &notes_shapes {
            write_shape(&mut out, &mut reg, shape, true);
        }
        out.push('\n');
    }
    std::fs::write(slides_dir.join(format!("slide{n}.md")), out)?;
    Ok(())
}
