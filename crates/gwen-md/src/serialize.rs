use serde_json::{Map, Value};

/// Serialize a presentation snapshot (the JSON produced by `query_value`) into
/// the markdown mirror. The output is a faithful, editable representation of
/// every field the apply engine can change; read-only fields (shape ids,
/// placeholder flags, slide layout references, chart data, hyperlinks, ...)
/// are deliberately omitted and reconstructed from the original deck on apply.
pub fn serialize(snapshot: &Value) -> String {
    let mut out = String::new();
    let obj = snapshot.as_object().expect("snapshot is an object");

    let sw = obj.get("slide_width").map(scalar).unwrap_or_default();
    let sh = obj.get("slide_height").map(scalar).unwrap_or_default();
    out.push_str(&format!(
        "<!-- pptx: slide_width={sw} slide_height={sh} -->\n\n"
    ));

    write_core_properties(&mut out, obj.get("core_properties"));
    write_theme(&mut out, obj.get("theme"));

    if let Some(masters) = obj.get("slide_masters").and_then(Value::as_array) {
        for master in masters {
            out.push_str("<!-- master -->\n");
            if let Some(shapes) = master.get("shapes").and_then(Value::as_array) {
                for shape in shapes {
                    write_shape(&mut out, shape);
                }
            }
            out.push('\n');
        }
    }

    if let Some(slides) = obj.get("slides").and_then(Value::as_array) {
        for slide in slides {
            match slide_background(slide) {
                Some(bg) => out.push_str(&format!("<!-- slide: background={bg} -->\n")),
                None => out.push_str("<!-- slide -->\n"),
            }
            if let Some(shapes) = slide.get("shapes").and_then(Value::as_array) {
                for shape in shapes {
                    write_shape(&mut out, shape);
                }
            }
            if let Some(notes) = slide.get("notes")
                && !notes.is_null()
            {
                out.push_str("<!-- notes -->\n");
                if let Some(shapes) = notes.get("shapes").and_then(Value::as_array) {
                    for shape in shapes {
                        write_shape(&mut out, shape);
                    }
                }
                out.push('\n');
            }
            out.push('\n');
        }
    }

    out
}

fn write_core_properties(out: &mut String, core: Option<&Value>) {
    out.push_str("<!-- core_properties -->\n");
    if let Some(core) = core.and_then(Value::as_object) {
        out.push_str("| key | value |\n|---|---|\n");
        for (k, v) in core {
            out.push_str(&format!(
                "| {} | {} |\n",
                escape_text(k),
                escape_text(&scalar(v))
            ));
        }
    }
    out.push('\n');
}

fn write_theme(out: &mut String, theme: Option<&Value>) {
    let Some(theme) = theme.and_then(Value::as_object) else {
        return;
    };
    if let Some(colors) = theme.get("colors").and_then(Value::as_object) {
        out.push_str("<!-- theme_colors -->\n| color | value |\n|---|---|\n");
        for (k, v) in colors {
            out.push_str(&format!(
                "| {} | {} |\n",
                escape_text(k),
                escape_text(&scalar(v))
            ));
        }
        out.push('\n');
    }
    if let Some(fonts) = theme.get("fonts").and_then(Value::as_object) {
        out.push_str("<!-- theme_fonts -->\n| font | value |\n|---|---|\n");
        for (k, v) in fonts {
            out.push_str(&format!(
                "| {} | {} |\n",
                escape_text(k),
                escape_text(&scalar(v))
            ));
        }
        out.push('\n');
    }
}

fn write_shape(out: &mut String, shape: &Value) {
    let obj = shape.as_object().expect("shape is an object");

    let mut attrs = vec![format!(
        "type={}",
        obj.get("shape_type")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_ascii_lowercase()
    )];

    if let Some(name) = obj.get("name").and_then(Value::as_str) {
        attrs.push(format!("name={}", quote(name)));
    }
    for (short, long) in [("x", "left"), ("y", "top"), ("w", "width"), ("h", "height")] {
        if let Some(v) = obj.get(long)
            && !v.is_null()
        {
            attrs.push(format!("{short}={}", scalar(v)));
        }
    }
    if let Some(v) = obj.get("rotation")
        && !v.is_null()
    {
        attrs.push(format!("rotation={}", scalar(v)));
    }
    if let Some(v) = obj.get("auto_shape_type").and_then(Value::as_str) {
        attrs.push(format!("autoshape={}", quote(v)));
    }
    for field in ["fill", "outline", "crop"] {
        if let Some(v) = obj.get(field)
            && v.is_object()
        {
            attrs.push(format!("{field}={}", compact_json(v)));
        }
    }

    out.push_str(&format!("<!-- shape: {} -->\n", attrs.join(" ")));

    if let Some(tf) = obj.get("text_frame") {
        write_text_frame(out, tf);
    } else if let Some(table) = obj.get("table") {
        write_table(out, table);
    }
}

fn write_text_frame(out: &mut String, tf: &Value) {
    let obj = tf.as_object().expect("text_frame is an object");

    let mut attrs = Vec::new();
    for key in [
        "auto_size",
        "word_wrap",
        "vertical_anchor",
        "margin_left",
        "margin_right",
        "margin_top",
        "margin_bottom",
    ] {
        if let Some(v) = obj.get(key)
            && !v.is_null()
        {
            attrs.push(format!("{key}={}", attr_scalar(key, v)));
        }
    }

    let paragraphs = obj.get("paragraphs").and_then(Value::as_array);
    let has_paragraphs = paragraphs.is_some_and(|p| !p.is_empty());
    let has_dps = obj
        .get("default_paragraph_style")
        .is_some_and(|v| v.is_object());

    if !attrs.is_empty() {
        out.push_str(&format!("<!-- tf: {} -->\n", attrs.join(" ")));
    }

    if has_dps {
        out.push_str("<!-- dp_style -->\n");
        write_para_comment(out, obj.get("default_paragraph_style").unwrap());
    }

    if let Some(paragraphs) = paragraphs {
        for para in paragraphs {
            write_paragraph(out, para);
        }
    }

    if attrs.is_empty() && !has_paragraphs && !has_dps {
        out.push_str("<!-- tf -->\n");
    }
}

fn write_para_comment(out: &mut String, para: &Value) {
    let obj = para.as_object().expect("paragraph is an object");
    let attrs = para_attrs(obj);
    if !attrs.is_empty() {
        out.push_str(&format!("<!-- para: {} -->\n", attrs.join(" ")));
    }
}

fn write_paragraph(out: &mut String, para: &Value) {
    let obj = para.as_object().expect("paragraph is an object");
    write_para_comment(out, para);

    let content = write_runs(
        obj.get("runs")
            .and_then(Value::as_array)
            .unwrap_or(&Vec::new()),
    );
    if content.is_empty() {
        out.push_str("<span></span>\n");
    } else {
        out.push_str(&content);
        out.push('\n');
    }
    out.push('\n');
}

/// All paragraph style attributes emitted into a `<!-- para: -->` comment.
fn para_attrs(obj: &Map<String, Value>) -> Vec<String> {
    let mut attrs = Vec::new();
    if let Some(v) = obj.get("alignment").and_then(Value::as_str) {
        attrs.push(format!("alignment={}", v.to_ascii_lowercase()));
    }
    if let Some(level) = obj.get("level").and_then(Value::as_i64)
        && level != 0
    {
        attrs.push(format!("level={level}"));
    }
    if let Some(v) = obj.get("line_spacing")
        && !v.is_null()
    {
        attrs.push(format!("line_spacing={}", scalar(v)));
    }
    for key in ["space_before", "space_after"] {
        if let Some(v) = obj.get(key)
            && !v.is_null()
        {
            attrs.push(format!("{key}={}", scalar(v)));
        }
    }
    if let Some(font) = obj.get("font").and_then(Value::as_object) {
        attrs.extend(font_para_attrs(font));
    }
    attrs
}

/// Font attributes for a `<!-- para: -->` comment (plain `key=value` pairs,
/// distinct from the span `data-*` attributes runs use).
fn font_para_attrs(font: &Map<String, Value>) -> Vec<String> {
    let mut attrs = Vec::new();
    if let Some(v) = font.get("size")
        && !v.is_null()
    {
        attrs.push(format!("font_size={}", scalar(v)));
    }
    if let Some(v) = font.get("name").and_then(Value::as_str) {
        attrs.push(format!("font_name={}", quote(v)));
    }
    for (key, field) in [
        ("bold", "font_bold"),
        ("italic", "font_italic"),
        ("underline", "font_underline"),
    ] {
        if let Some(v) = font.get(key).and_then(Value::as_bool) {
            attrs.push(format!("{field}={}", if v { "true" } else { "false" }));
        }
    }
    if let Some(color) = font.get("color").and_then(Value::as_object) {
        attrs.push(format!("font_color=\"{}\"", color_value(color)));
    }
    attrs
}

fn write_runs(runs: &[Value]) -> String {
    let mut out = String::new();
    let mut prev_plain = false;
    for run in runs {
        let obj = run.as_object().expect("run is an object");
        let text = obj.get("text").and_then(Value::as_str).unwrap_or("");
        let font = obj.get("font").filter(|f| f.is_object());

        let plain = font.is_none();
        if plain && prev_plain {
            // Adjacent plain runs are indistinguishable in markdown; an empty
            // span marks the run boundary.
            out.push_str("<span></span>");
        }
        out.push_str(&write_run(text, font));
        prev_plain = plain;
    }
    out
}

/// Serialize a single run: bare text, native emphasis, or a `<span>` carrying
/// the font attributes when the run cannot be expressed with emphasis alone.
fn write_run(text: &str, font: Option<&Value>) -> String {
    let attrs = font
        .map(|f| font_attrs(f.as_object().unwrap()))
        .unwrap_or_default();
    if attrs.is_empty() {
        let s = escape_text(text);
        if let Some(font) = font {
            let obj = font.as_object().unwrap();
            let bold = obj.get("bold").and_then(Value::as_bool).unwrap_or(false);
            let italic = obj.get("italic").and_then(Value::as_bool).unwrap_or(false);
            match (bold, italic) {
                (true, true) => return format!("***{s}***"),
                (true, false) => return format!("**{s}**"),
                (false, true) => return format!("*{s}*"),
                (false, false) => return s,
            }
        }
        return s;
    }
    if let Some(font) = font {
        let obj = font.as_object().unwrap();
        let bold = obj.get("bold").and_then(Value::as_bool).unwrap_or(false);
        let italic = obj.get("italic").and_then(Value::as_bool).unwrap_or(false);
        let mut all = attrs.clone();
        if bold {
            all.push("data-bold=\"true\"".to_string());
        }
        if italic {
            all.push("data-italic=\"true\"".to_string());
        }
        return format!("<span {}>{}</span>", all.join(" "), escape_text(text));
    }
    format!("<span {}>{}</span>", attrs.join(" "), escape_text(text))
}

/// Font attributes as `<span data-*>` attributes. Emits only present fields.
fn font_attrs(font: &Map<String, Value>) -> Vec<String> {
    let mut attrs = Vec::new();
    if let Some(v) = font.get("size")
        && !v.is_null()
    {
        attrs.push(format!("data-size={}", scalar(v)));
    }
    if let Some(v) = font.get("name").and_then(Value::as_str) {
        attrs.push(format!("data-name={}", quote(v)));
    }
    if let Some(v) = font.get("underline").and_then(Value::as_bool) {
        attrs.push(format!("data-underline=\"{v}\""));
    }
    if let Some(color) = font.get("color").and_then(Value::as_object) {
        attrs.push(format!("data-color=\"{}\"", color_value(color)));
    }
    attrs
}

/// Encode a `ColorFormatDto`-shaped object as `TYPE:VALUE`.
fn color_value(color: &Map<String, Value>) -> String {
    let ty = color.get("type").and_then(Value::as_str).unwrap_or("");
    let value = color
        .get("rgb")
        .or_else(|| color.get("theme_color"))
        .and_then(Value::as_str)
        .unwrap_or("");
    format!("{ty}:{value}")
}

fn write_table(out: &mut String, table: &Value) {
    let obj = table.as_object().expect("table is an object");
    let widths = obj
        .get("grid")
        .and_then(Value::as_array)
        .map(|g| {
            g.iter()
                .filter_map(|c| c.get("width").and_then(Value::as_i64))
                .map(|w| w.to_string())
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();
    out.push_str(&format!("<!-- table: grid={widths} -->\n"));

    let rows = obj.get("rows").and_then(Value::as_array);
    let Some(rows) = rows else { return };
    let cols = rows
        .first()
        .and_then(|r| r.get("cells").and_then(Value::as_array))
        .map(|c| c.len())
        .unwrap_or(0);

    let mut header = vec!["".to_string(); cols];
    if let Some(first) = rows.first()
        && let Some(cells) = first.get("cells").and_then(Value::as_array)
    {
        header = cells.iter().map(write_cell).collect();
    }
    out.push_str(&format!("| {} |\n", header.join(" | ")));
    out.push_str(&format!("| {} |\n", vec!["---"; cols].join(" | ")));

    for row in rows.iter().skip(1) {
        let cells = row
            .get("cells")
            .and_then(Value::as_array)
            .map(|c| c.iter().map(write_cell).collect::<Vec<_>>())
            .unwrap_or_default();
        out.push_str(&format!("| {} |\n", cells.join(" | ")));
    }
    out.push('\n');
}

fn write_cell(cell: &Value) -> String {
    let Some(tf) = cell.get("text_frame").and_then(Value::as_object) else {
        return String::new();
    };
    let paragraphs = tf.get("paragraphs").and_then(Value::as_array);
    let Some(paragraphs) = paragraphs else {
        return String::new();
    };
    paragraphs
        .iter()
        .map(|p| {
            write_runs(
                p.get("runs")
                    .and_then(Value::as_array)
                    .unwrap_or(&Vec::new()),
            )
        })
        .collect::<Vec<_>>()
        .join("<br>")
}

fn attr_scalar(key: &str, v: &Value) -> String {
    match key {
        "auto_size" | "vertical_anchor" => v
            .as_str()
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default(),
        "word_wrap" => match v.as_bool() {
            Some(true) => "1".into(),
            Some(false) => "0".into(),
            None => scalar(v),
        },
        _ => scalar(v),
    }
}

fn scalar(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn compact_json(v: &Value) -> String {
    serde_json::to_string(v).expect("serializable JSON value")
}

/// The slide background as a `TYPE:COLOR` attribute, or `None` when the slide
/// has no background element (`fill.type` is null).
fn slide_background(slide: &Value) -> Option<String> {
    let fill = slide.get("background")?.get("fill")?;
    let ty = fill.get("type").and_then(Value::as_str)?;
    if ty.is_empty() {
        return None;
    }
    let color = fill.get("color").and_then(Value::as_str).unwrap_or("");
    Some(format!("{ty}:{color}"))
}

/// Quote a value for use inside an HTML comment attribute. Backslashes and
/// quote characters are backslash-escaped; `--` is escaped so it can never
/// terminate the comment early.
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '-' => {
                if out.ends_with('-') {
                    out.push_str("\\-");
                } else {
                    out.push('-');
                }
            }
            _ => out.push(c),
        }
    }
    out.push('"');
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
