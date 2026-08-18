use serde_json::{Map, Value};

use super::markers::{
    ATTR_CROP_PREFIX, ATTR_GRID, ATTR_NAME, ATTR_ROTATION, LENGTH_ATTRS, parse_length,
};

/// A single CSS declaration, e.g. `left: 914400`.
#[derive(Debug, Clone, PartialEq)]
pub struct Decl {
    pub prop: String,
    pub value: String,
}

impl Decl {
    fn new(prop: &str, value: impl Into<String>) -> Decl {
        Decl {
            prop: prop.to_string(),
            value: value.into(),
        }
    }
}

/// The parsed `<style>` block: class name -> declarations.
pub type Styles = std::collections::HashMap<String, Vec<Decl>>;

// ---------------------------------------------------------------------------
// Serializer-side: JSON -> declarations, with class assignment
// ---------------------------------------------------------------------------

/// Registry assigning deduplicated, kind-derived class names to identical
/// style declaration sets. Counters run per kind in first-appearance order.
pub struct StyleRegistry {
    classes: std::collections::HashMap<(String, String), String>,
    counters: std::collections::HashMap<String, u32>,
    rules: Vec<(String, Vec<Decl>)>,
}

impl Default for StyleRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl StyleRegistry {
    pub fn new() -> Self {
        StyleRegistry {
            classes: std::collections::HashMap::new(),
            counters: std::collections::HashMap::new(),
            rules: Vec::new(),
        }
    }

    /// Return the class for `kind` with `decls`, creating and registering it on
    /// first appearance.
    pub fn class_for(&mut self, kind: &str, decls: &[Decl]) -> String {
        let key = decl_key(decls);
        if let Some(name) = self.classes.get(&(kind.to_string(), key.clone())) {
            return name.clone();
        }
        let n = self.counters.entry(kind.to_string()).or_insert(0);
        *n += 1;
        let name = format!("{kind}-{n}");
        self.classes.insert((kind.to_string(), key), name.clone());
        self.rules.push((name.clone(), decls.to_vec()));
        name
    }

    /// Render the `<style>` block for every registered class.
    pub fn to_style_block(&self) -> String {
        let mut out = String::new();
        out.push_str("<style>\n");
        for (name, decls) in &self.rules {
            out.push_str(&format!(".{name} {{\n"));
            for d in decls {
                out.push_str(&format!("    {}: {};\n", d.prop, d.value));
            }
            out.push_str("}\n");
        }
        out.push_str("</style>\n");
        out
    }
}

fn decl_key(decls: &[Decl]) -> String {
    let mut parts: Vec<String> = decls
        .iter()
        .map(|d| format!("{}:{}", d.prop, d.value))
        .collect();
    parts.sort();
    parts.join(";")
}

/// The kind prefix for a shape's class, derived from its type and (for auto
/// shapes) geometry type.
pub fn shape_kind(shape_type: &str, auto_shape_type: Option<&str>) -> String {
    match shape_type {
        "PLACEHOLDER" => "placeholder".to_string(),
        "PICTURE" => "pic".to_string(),
        "TABLE" => "table".to_string(),
        "GROUP" => "group".to_string(),
        "CHART" => "chart".to_string(),
        "TEXT_BOX" => "textbox".to_string(),
        _ => auto_shape_type
            .map(|a| a.to_ascii_lowercase())
            .unwrap_or_else(|| shape_type.to_ascii_lowercase()),
    }
}

fn is_null(v: &Value) -> bool {
    v.is_null()
}

fn val<'a>(obj: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    obj.get(key).filter(|v| !is_null(v))
}

/// CSS declarations for a shape. Styling only: identity lives in the shape
/// marker's `type=`/`auto-shape=` attributes and geometry in its `name`,
/// `left/top/width/height`, `rotation`, `grid` and `crop-*` attributes.
pub fn shape_decls(shape: &Map<String, Value>) -> Vec<Decl> {
    let mut out = Vec::new();
    if let Some(fill) = val(shape, "fill").and_then(Value::as_object) {
        push_fill_decl(&mut out, "fill", fill);
    }
    if let Some(outline) = val(shape, "outline").and_then(Value::as_object) {
        push_outline_decls(&mut out, outline);
    }
    out
}

fn push_fill_decl(out: &mut Vec<Decl>, prop: &str, fill: &Map<String, Value>) {
    let ty = fill.get("type").and_then(Value::as_str).unwrap_or("");
    match ty.to_ascii_lowercase().as_str() {
        "no_fill" => out.push(Decl::new(prop, "none")),
        "solid" => {
            if let Some(color) = fill.get("color").and_then(Value::as_object) {
                out.push(Decl::new(prop, color_token(color)));
            } else {
                out.push(Decl::new(prop, "solid"));
            }
        }
        _ => {
            if let Some(color) = fill.get("color").and_then(Value::as_object) {
                out.push(Decl::new(prop, color_token(color)));
            }
        }
    }
}

fn push_outline_decls(out: &mut Vec<Decl>, outline: &Map<String, Value>) {
    if let Some(v) = val(outline, "width") {
        out.push(Decl::new("outline-width", scalar(v)));
    }
    for (key, decl) in [
        ("cap", "--pptx-outline-cap"),
        ("compound", "--pptx-outline-compound"),
        ("dash", "--pptx-outline-dash"),
    ] {
        if let Some(v) = val(outline, key).and_then(Value::as_str) {
            out.push(Decl::new(decl, v.to_ascii_lowercase()));
        }
    }
    if let Some(fill) = val(outline, "fill").and_then(Value::as_object) {
        push_fill_decl(out, "outline", fill);
    }
}

/// `RGB(FF0000)` / `SCHEME(tx1)` from a `ColorFormatDto`-shaped object.
fn color_token(color: &Map<String, Value>) -> String {
    let ty = color.get("type").and_then(Value::as_str).unwrap_or("");
    let value = color
        .get("rgb")
        .or_else(|| color.get("theme_color"))
        .and_then(Value::as_str)
        .unwrap_or("");
    format!("{}({})", ty.to_ascii_uppercase(), value)
}

/// CSS declarations for a text frame body (`a:bodyPr`) properties.
pub fn tf_decls(tf: &Map<String, Value>) -> Vec<Decl> {
    let mut out = Vec::new();
    if let Some(v) = val(tf, "auto_size").and_then(Value::as_str) {
        out.push(Decl::new("--pptx-auto-size", v.to_ascii_lowercase()));
    }
    if let Some(v) = val(tf, "vertical_anchor").and_then(Value::as_str) {
        out.push(Decl::new("--pptx-vertical-anchor", v.to_ascii_lowercase()));
    }
    if let Some(v) = val(tf, "word_wrap") {
        out.push(Decl::new(
            "white-space",
            if v.as_bool().unwrap_or(false) {
                "normal"
            } else {
                "nowrap"
            },
        ));
    }
    for key in ["margin_top", "margin_right", "margin_bottom", "margin_left"] {
        if let Some(v) = val(tf, key) {
            out.push(Decl::new(&key.replace('_', "-"), scalar(v)));
        }
    }
    out
}

/// CSS declarations for a paragraph's own style (`a:pPr`), shared by the
/// `default_paragraph_style` (`a:lstStyle`).
pub fn para_decls(para: &Map<String, Value>) -> Vec<Decl> {
    let mut out = Vec::new();
    if let Some(v) = val(para, "alignment").and_then(Value::as_str) {
        out.push(Decl::new("text-align", v.to_ascii_lowercase()));
    }
    if let Some(level) = val(para, "level").and_then(Value::as_i64)
        && level != 0
    {
        out.push(Decl::new("--pptx-level", level.to_string()));
    }
    if let Some(v) = val(para, "line_spacing") {
        out.push(Decl::new("line-height", scalar(v)));
    }
    for (field, decl) in [
        ("space_before", "--pptx-space-before"),
        ("space_after", "--pptx-space-after"),
    ] {
        if let Some(v) = val(para, field) {
            out.push(Decl::new(decl, scalar(v)));
        }
    }
    if let Some(font) = val(para, "font").and_then(Value::as_object) {
        push_font_decls(&mut out, font);
    }
    out
}

/// CSS declarations for a run's font (or a paragraph's `endParaRPr` font).
fn push_font_decls(out: &mut Vec<Decl>, font: &Map<String, Value>) {
    if let Some(v) = val(font, "size") {
        out.push(Decl::new("font-size", scalar(v)));
    }
    if let Some(v) = val(font, "name").and_then(Value::as_str) {
        out.push(Decl::new("font-family", quote(v)));
    }
    for (key, decl) in [
        ("bold", "font-weight"),
        ("italic", "font-style"),
        ("underline", "text-decoration"),
    ] {
        if let Some(v) = val(font, key) {
            let (on, off) = match key {
                "bold" => ("bold", "normal"),
                "italic" => ("italic", "normal"),
                _ => ("underline", "none"),
            };
            out.push(Decl::new(
                decl,
                if v.as_bool().unwrap_or(false) {
                    on
                } else {
                    off
                },
            ));
        }
    }
    if let Some(color) = val(font, "color").and_then(Value::as_object) {
        out.push(Decl::new("color", color_token(color)));
    }
}

/// CSS declarations for a run's font.
pub fn run_decls(font: &Map<String, Value>) -> Vec<Decl> {
    let mut out = Vec::new();
    push_font_decls(&mut out, font);
    out
}

// ---------------------------------------------------------------------------
// Parser-side: declarations -> JSON
// ---------------------------------------------------------------------------

/// A `--pptx-*` / geometry declaration value with its raw string token.
fn decl_str<'a>(decls: &'a [Decl], prop: &str) -> Option<&'a str> {
    decls
        .iter()
        .find(|d| d.prop == prop)
        .map(|d| d.value.as_str())
}

fn decl_i64(decls: &[Decl], prop: &str) -> Option<i64> {
    decl_str(decls, prop).and_then(|v| v.parse().ok())
}

fn decl_f64(decls: &[Decl], prop: &str) -> Option<f64> {
    decl_str(decls, prop).and_then(|v| v.parse().ok())
}

/// Whether a class carries paragraph-level declarations (a default paragraph
/// style folded into the shape class). Every text frame with a
/// `default_paragraph_style` folds those declarations into its shape class, so
/// their presence is exactly the presence of the default style.
pub fn has_para_decls(decls: &[Decl]) -> bool {
    decls.iter().any(|d| {
        matches!(
            d.prop.as_str(),
            "text-align"
                | "--pptx-level"
                | "line-height"
                | "--pptx-space-before"
                | "--pptx-space-after"
                | "font-size"
                | "font-family"
                | "font-weight"
                | "font-style"
                | "text-decoration"
                | "color"
        )
    })
}

/// Rebuild a shape object from its style declarations. Fill and outline only:
/// the shape's `shape_type`/`auto_shape_type` come from the marker's
/// `type=`/`auto-shape=` attributes and its geometry from `shape_attrs`.
pub fn shape_from_decls(decls: &[Decl]) -> Map<String, Value> {
    let mut shape = Map::new();
    if let Some(fill) = fill_from_decl(decl_str(decls, "fill")) {
        shape.insert("fill".into(), fill);
    }
    if let Some(outline) = outline_from_decls(decls) {
        shape.insert("outline".into(), outline);
    }
    shape
}

/// Apply the `name`, `left/top/width/height`, `rotation`, `grid` and
/// `crop-*` attributes from a shape marker onto a shape object. Attributes are
/// only present when the value was non-null in the source snapshot. Lengths
/// accept raw EMU or a unit suffix (`1in`, `3cm`, `25mm`, `72pt`, `96px`).
pub fn shape_attrs(shape: &mut Map<String, Value>, attrs: &[(String, String)]) {
    for (key, value) in attrs {
        match key.as_str() {
            ATTR_NAME => {
                shape.insert("name".into(), Value::String(value.clone()));
            }
            key if LENGTH_ATTRS.contains(&key) => {
                if let Some(v) = parse_length(value) {
                    shape.insert(key.to_string(), Value::from(v));
                }
            }
            ATTR_ROTATION => {
                if let Ok(v) = value.parse::<f64>() {
                    shape.insert("rotation".into(), Value::from(v));
                }
            }
            ATTR_GRID => {
                // Consumed by the table builder, which needs the raw widths.
            }
            _ => {
                if let Some(side) = key.strip_prefix(ATTR_CROP_PREFIX)
                    && let Ok(v) = value.parse::<f64>()
                    && matches!(side, "left" | "top" | "right" | "bottom")
                {
                    let crop = shape
                        .entry("crop")
                        .or_insert_with(|| Value::Object(Map::new()))
                        .as_object_mut()
                        .expect("crop is an object");
                    crop.insert(side.into(), Value::from(v));
                }
            }
        }
    }
}

fn fill_from_decl(value: Option<&str>) -> Option<Value> {
    let v = value?;
    match v {
        "none" => Some(json_fill("no_fill", None)),
        "solid" => Some(json_fill("solid", None)),
        _ => {
            let (ty, inner) = parse_color_token(v);
            match ty.as_str() {
                "RGB" | "SCHEME" => Some(json_fill("solid", Some(color_value(&ty, &inner)))),
                _ => {
                    let lower = ty.to_ascii_lowercase();
                    Some(json_fill(&lower, None))
                }
            }
        }
    }
}

fn json_fill(ty: &str, color: Option<Value>) -> Value {
    let mut m = Map::new();
    m.insert("type".into(), Value::String(ty.to_ascii_lowercase()));
    if let Some(color) = color {
        m.insert("color".into(), color);
    }
    Value::Object(m)
}

/// A `{type, rgb|theme_color}` color object from a `TYPE(VALUE)` token.
fn color_value(ty: &str, inner: &str) -> Value {
    let mut m = Map::new();
    m.insert("type".into(), Value::String(ty.to_ascii_uppercase()));
    if ty.eq_ignore_ascii_case("RGB") {
        m.insert("rgb".into(), Value::String(inner.to_string()));
    } else if ty.eq_ignore_ascii_case("SCHEME") {
        m.insert("theme_color".into(), Value::String(inner.to_string()));
    }
    Value::Object(m)
}

fn parse_color_token(s: &str) -> (String, String) {
    match s.find('(') {
        Some(i) if s.ends_with(')') => (s[..i].to_string(), s[i + 1..s.len() - 1].to_string()),
        _ => (s.to_string(), String::new()),
    }
}

fn outline_from_decls(decls: &[Decl]) -> Option<Value> {
    let width = decl_i64(decls, "outline-width");
    let cap = decl_str(decls, "--pptx-outline-cap");
    let compound = decl_str(decls, "--pptx-outline-compound");
    let dash = decl_str(decls, "--pptx-outline-dash");
    let fill = fill_from_decl(decl_str(decls, "outline"));
    if width.is_none() && cap.is_none() && compound.is_none() && dash.is_none() && fill.is_none() {
        return None;
    }
    let mut m = Map::new();
    if let Some(w) = width {
        m.insert("width".into(), Value::from(w));
    }
    if let Some(v) = cap {
        m.insert("cap".into(), Value::String(v.to_ascii_lowercase()));
    }
    if let Some(v) = compound {
        m.insert("compound".into(), Value::String(v.to_ascii_lowercase()));
    }
    if let Some(v) = dash {
        m.insert("dash".into(), Value::String(v.to_ascii_lowercase()));
    }
    if let Some(v) = fill {
        m.insert("fill".into(), v);
    }
    Some(Value::Object(m))
}

/// Rebuild a text frame body-properties object from its style declarations.
pub fn tf_from_decls(decls: &[Decl]) -> Map<String, Value> {
    let mut tf = Map::new();
    if let Some(v) = decl_str(decls, "--pptx-auto-size") {
        tf.insert("auto_size".into(), Value::String(v.to_ascii_uppercase()));
    }
    if let Some(v) = decl_str(decls, "--pptx-vertical-anchor") {
        tf.insert(
            "vertical_anchor".into(),
            Value::String(v.to_ascii_uppercase()),
        );
    }
    if let Some(v) = decl_str(decls, "white-space") {
        tf.insert("word_wrap".to_string(), Value::Bool(v != "nowrap"));
    }
    for key in ["margin-top", "margin-right", "margin-bottom", "margin-left"] {
        if let Some(v) = decl_i64(decls, key) {
            tf.insert(key.replace('-', "_"), Value::from(v));
        }
    }
    tf
}

/// Rebuild a paragraph style object from its style declarations (used for both
/// the paragraph's own `a:pPr` and the `default_paragraph_style`).
pub fn para_from_decls(decls: &[Decl]) -> Map<String, Value> {
    let mut para = Map::new();
    if let Some(v) = decl_str(decls, "text-align") {
        para.insert("alignment".into(), Value::String(v.to_ascii_uppercase()));
    }
    let level = decl_i64(decls, "--pptx-level").unwrap_or(0);
    para.insert("level".into(), Value::from(level));
    if let Some(v) = decl_f64(decls, "line-height") {
        para.insert("line_spacing".into(), Value::from(v));
    }
    for (decl, field) in [
        ("--pptx-space-before", "space_before"),
        ("--pptx-space-after", "space_after"),
    ] {
        if let Some(v) = decl_i64(decls, decl) {
            para.insert(field.into(), Value::from(v));
        }
    }
    if let Some(font) = font_from_decls(decls) {
        para.insert("font".into(), Value::Object(font));
    }
    para
}

/// Rebuild a run font object from its style declarations.
pub fn font_from_decls(decls: &[Decl]) -> Option<Map<String, Value>> {
    let size = decl_i64(decls, "font-size");
    let name = decl_str(decls, "font-family").map(unquote);
    let bold = decl_str(decls, "font-weight").map(|v| v == "bold");
    let italic = decl_str(decls, "font-style").map(|v| v == "italic");
    let underline = decl_str(decls, "text-decoration").map(|v| v == "underline");
    let color = decl_str(decls, "color")
        .map(parse_color_token)
        .and_then(|(ty, inner)| {
            if ty.eq_ignore_ascii_case("RGB") || ty.eq_ignore_ascii_case("SCHEME") {
                Some(color_value(&ty, &inner))
            } else {
                None
            }
        });
    if size.is_none()
        && name.is_none()
        && bold.is_none()
        && italic.is_none()
        && underline.is_none()
        && color.is_none()
    {
        return None;
    }
    let mut m = Map::new();
    if let Some(v) = size {
        m.insert("size".into(), Value::from(v));
    }
    if let Some(v) = name {
        m.insert("name".into(), Value::String(v));
    }
    if let Some(v) = bold {
        m.insert("bold".into(), Value::Bool(v));
    }
    if let Some(v) = italic {
        m.insert("italic".into(), Value::Bool(v));
    }
    if let Some(v) = underline {
        m.insert("underline".into(), Value::Bool(v));
    }
    if let Some(v) = color {
        m.insert("color".into(), v);
    }
    Some(m)
}

// ---------------------------------------------------------------------------
// Style block parsing
// ---------------------------------------------------------------------------

/// Parse a `<style>...</style>` block into class -> declarations. Malformed
/// rules are skipped; unknown class references are reported by the caller.
pub fn parse_style_block(html: &str) -> Styles {
    let inner = strip_style_tags(html);
    let mut rules = Styles::new();
    let mut chars: Vec<char> = inner.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        if chars[i] != '.' {
            // Skip unrecognized content up to the next `.` selector.
            i += 1;
            continue;
        }
        i += 1;
        let name_start = i;
        while i < chars.len() && is_ident_char(chars[i]) {
            i += 1;
        }
        let name: String = chars[name_start..i].iter().collect();
        while i < chars.len() && chars[i] != '{' {
            if chars[i] == '}' {
                i += 1;
                break;
            }
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        i += 1; // past `{`
        let decls = parse_decls(&mut chars, &mut i);
        if !name.is_empty() {
            rules.insert(name, decls);
        }
    }
    rules
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '-' || c == '_'
}

fn parse_decls(chars: &mut [char], i: &mut usize) -> Vec<Decl> {
    let mut out = Vec::new();
    loop {
        while *i < chars.len() && chars[*i].is_whitespace() {
            *i += 1;
        }
        if *i >= chars.len() {
            break;
        }
        if chars[*i] == '}' {
            *i += 1;
            break;
        }
        let mut prop = String::new();
        while *i < chars.len() && chars[*i] != ':' && chars[*i] != '}' {
            if !chars[*i].is_whitespace() {
                prop.push(chars[*i]);
            }
            *i += 1;
        }
        if *i >= chars.len() || chars[*i] != ':' {
            break;
        }
        *i += 1;
        let mut value = String::new();
        let mut in_quote = false;
        while *i < chars.len() {
            let c = chars[*i];
            if in_quote {
                if c == '\\' && *i + 1 < chars.len() {
                    value.push(c);
                    *i += 1;
                    value.push(chars[*i]);
                } else if c == '"' {
                    in_quote = false;
                    value.push(c);
                } else {
                    value.push(c);
                }
                *i += 1;
                continue;
            }
            if c == '"' {
                in_quote = true;
                value.push(c);
                *i += 1;
            } else if c == ';' || c == '}' {
                *i += 1;
                break;
            } else {
                value.push(c);
                *i += 1;
            }
        }
        if !prop.is_empty() {
            out.push(Decl::new(&prop, value.trim().to_string()));
        }
    }
    out
}

fn strip_style_tags(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<style").map(|i| {
        let after = &html[i + 6..];
        match after.find('>') {
            Some(j) => i + 6 + j + 1,
            None => html.len(),
        }
    });
    let start = start.unwrap_or(0);
    let end = lower[start..]
        .find("</style")
        .map(|i| start + i)
        .unwrap_or(html.len());
    html[start..end].to_string()
}

// ---------------------------------------------------------------------------
// Value helpers
// ---------------------------------------------------------------------------

pub fn scalar(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Quote a value for use inside a CSS declaration. Backslashes and double
/// quotes are backslash-escaped.
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn unquote(v: &str) -> String {
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        let inner = &v[1..v.len() - 1];
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some(next) => out.push(next),
                    None => out.push('\\'),
                }
            } else {
                out.push(c);
            }
        }
        out
    } else {
        v.to_string()
    }
}
