//! Native port of the pptxgenjs 4.0.1 emitter gwen used to run on QuickJS.
//!
//! The public input is the same JSON spec produced by [`crate::render`].  The
//! code deliberately follows pptxgenjs function-for-function (including its
//! defaults, XML whitespace and small quirks) so the parts it emits are the
//! same parts gwen has always packaged.  Everything here is plain Rust: there
//! is no JavaScript runtime or pptx library at run time.
//!
//! This module is derived from pptxgenjs, which is MIT licensed; see
//! [`LICENSE-PPTXGENJS`](./LICENSE-PPTXGENJS).

mod emit;

use std::collections::HashSet;

use base64::Engine;
use miette::{Result, miette};
use serde_json::{Map, Value, json};

use crate::zip::{self, Entry};

pub(crate) use emit::*;

pub(crate) const EMU: f64 = 914_400.0;
pub(crate) const ONEPT: f64 = 12_700.0;
pub(crate) const CRLF: &str = "\r\n";
pub(crate) const LAYOUT_IDX_SERIES_BASE: u64 = 2_147_483_649;
pub(crate) const DEF_FONT_COLOR: &str = "000000";
pub(crate) const DEF_SHAPE_LINE_COLOR: &str = "333333";
pub(crate) const DEF_BULLET_MARGIN: f64 = 27.0;
pub(crate) const SLDNUMFLDID: &str = "{F7021451-1387-4CA6-816F-3879F97B5CBC}";

pub(crate) type Opts = Map<String, Value>;

pub(crate) fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(_)) => true,
    }
}

pub(crate) fn js_number(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        Value::String(s) => s.trim().parse::<f64>().unwrap_or(f64::NAN),
        Value::Bool(true) => 1.0,
        Value::Bool(false) | Value::Null => 0.0,
        Value::Array(a) if a.is_empty() => 0.0,
        Value::Array(a) if a.len() == 1 => js_number(&a[0]),
        _ => f64::NAN,
    }
}

pub(crate) fn js_to_string(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => js_number_to_string(n.as_f64().unwrap_or(0.0)),
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().map(js_to_string).collect::<Vec<_>>().join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

/// `String(number)` in JavaScript.  In particular, integral floats lose their
/// `.0` suffix (serde_json keeps it when a float was deserialised).
fn js_number_to_string(n: f64) -> String {
    if n == 0.0 {
        "0".into()
    } else if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e21 {
        format!("{n:.0}")
    } else {
        n.to_string()
    }
}

pub(crate) fn encode_xml(v: &Value) -> String {
    encode_xml_str(&js_to_string(v))
}

pub(crate) fn encode_xml_str(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub(crate) fn opt_string(m: &Opts, key: &str) -> Option<String> {
    m.get(key).filter(|v| truthy(Some(v))).map(js_to_string)
}

pub(crate) fn opt_num(m: &Opts, key: &str) -> Option<f64> {
    m.get(key).filter(|v| truthy(Some(v))).and_then(|v| {
        let n = js_number(v);
        if n.is_nan() { None } else { Some(n) }
    })
}

pub(crate) fn opt_bool(m: &Opts, key: &str) -> bool {
    truthy(m.get(key))
}

pub(crate) fn opt_obj<'a>(m: &'a Opts, key: &str) -> Option<&'a Opts> {
    m.get(key).and_then(|v| v.as_object())
}

/// `Math.round`: JS rounds halves towards +∞ (Rust rounds away from zero).
pub(crate) fn js_round(n: f64) -> f64 {
    if n.is_nan() || n.is_infinite() {
        return n;
    }
    (n + 0.5).floor()
}

pub(crate) fn inch2emu(v: &Value) -> f64 {
    if let Value::Number(n) = v
        && n.as_f64().unwrap_or(0.0) > 100.0
    {
        return n.as_f64().unwrap_or(0.0);
    }
    let num = match v {
        Value::String(s) => {
            let cleaned: String = s
                .chars()
                .filter(|c| *c != 'i' && *c != 'I' && *c != 'n' && *c != 'N')
                .collect();
            cleaned.trim().parse::<f64>().unwrap_or(0.0)
        }
        other => js_number(other),
    };
    js_round(EMU * num)
}

pub(crate) fn val_to_pts(v: &Value) -> f64 {
    let n = js_number(v);
    if n.is_nan() { 0.0 } else { js_round(n * ONEPT) }
}

pub(crate) fn convert_rotation_degrees(v: &Value) -> f64 {
    let mut d = js_number(v);
    if d.is_nan() {
        d = 0.0;
    }
    js_round((if d > 360.0 { d - 360.0 } else { d }) * 60_000.0)
}

/// pptxgenjs's `getSmartParseNumber`.  `layout` is `(width_emu, height_emu)`.
pub(crate) fn smart_number(v: &Value, axis: char, layout: (f64, f64)) -> f64 {
    let mut n = v.clone();
    if let Value::String(s) = &n
        && let Ok(parsed) = s.parse::<f64>()
    {
        n = json!(parsed);
    }
    match &n {
        Value::Number(num) if num.as_f64().unwrap_or(0.0) < 100.0 => inch2emu(&n),
        Value::Number(num) => num.as_f64().unwrap_or(0.0),
        Value::String(s) if s.contains('%') => {
            let pct = s.trim_end_matches('%').parse::<f64>().unwrap_or(0.0);
            let base = if axis == 'Y' { layout.1 } else { layout.0 };
            js_round((pct / 100.0) * base)
        }
        _ => 0.0,
    }
}

pub(crate) fn get_new_rel_id(t: &Target) -> u64 {
    (t.rels.len() + t.media.len() + 1) as u64
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ObjKind {
    Text,
    Placeholder,
    Image,
    Notes,
}

#[derive(Clone, Debug)]
pub(crate) struct Run {
    pub text: String,
    pub options: Opts,
}

#[derive(Clone, Debug)]
pub(crate) struct Rel {
    pub r_id: u64,
    pub kind: String,
    pub data: Option<String>,
    pub target: String,
}

#[derive(Clone, Debug)]
pub(crate) struct MediaRel {
    pub r_id: u64,
    pub type_: String,
    pub extn: String,
    pub data_uri: Option<String>,
    pub target: String,
}

#[derive(Clone, Debug)]
pub(crate) struct Obj {
    pub kind: ObjKind,
    pub options: Opts,
    pub text: Option<Vec<Run>>,
    pub shape: String,
    /// pptxgenjs's `newObject.image` (used as the image `descr` fallback).
    pub image: Option<String>,
    pub image_rid: Option<u64>,
    pub hyperlink: Option<Opts>,
    pub has_custom_prompt: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Target {
    pub name: Option<String>,
    pub slide_num: Option<u64>,
    pub rels: Vec<Rel>,
    pub media: Vec<MediaRel>,
    pub objects: Vec<Obj>,
    pub background: Option<Opts>,
    pub slide_number: Option<Opts>,
    pub slide_layout: Option<usize>,
}

#[derive(Clone, Debug)]
pub(crate) struct PlaceholderInfo {
    pub options: Opts,
    pub has_custom_prompt: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Section {
    pub title: String,
    pub slide_ids: Vec<usize>,
}

pub(crate) struct Deck {
    pub width: f64,
    pub height: f64,
    pub major_font: String,
    pub minor_font: String,
    pub layouts: Vec<Target>,
    pub master: Target,
    pub slides: Vec<Target>,
    pub sections: Vec<Section>,
    pub section_uuids: Vec<String>,
    /// Final SVG preview for each SVG data URI.  pptxgenjs resolves previews
    /// while packaging, after the bridge has registered every shape, so a
    /// later occurrence of the same SVG wins over an earlier one.
    pub svg_previews: std::collections::HashMap<String, String>,
}

impl Deck {
    pub fn layout_size(&self) -> (f64, f64) {
        (self.width, self.height)
    }

    pub fn layout_placeholders(&self, layout: usize) -> Vec<PlaceholderInfo> {
        self.layouts
            .get(layout)
            .map(|l| {
                l.objects
                    .iter()
                    .filter(|o| o.kind == ObjKind::Placeholder)
                    .map(|o| PlaceholderInfo {
                        options: o.options.clone(),
                        has_custom_prompt: o.has_custom_prompt,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn layout_index(&self, name: &str) -> Option<usize> {
        self.layouts
            .iter()
            .position(|l| l.name.as_deref() == Some(name))
    }
}

struct Lcg {
    state: u32,
}

impl Lcg {
    fn new() -> Self {
        Self { state: 0x9e37_79b9 }
    }

    fn next(&mut self) -> f64 {
        self.state = self
            .state
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.state as f64 / 4_294_967_296.0
    }

    fn uuid(&mut self) -> String {
        let format = "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx";
        let mut out = String::with_capacity(36);
        for c in format.chars() {
            match c {
                'x' => {
                    let r = (self.next() * 16.0) as u32;
                    out.push(char::from_digit(r, 16).unwrap().to_ascii_uppercase());
                }
                'y' => {
                    let r = (self.next() * 16.0) as u32;
                    let v = 8 | (r & 3);
                    out.push(char::from_digit(v, 16).unwrap().to_ascii_uppercase());
                }
                other => out.push(other),
            }
        }
        out
    }
}

/// Parse a spec run list exactly like the old bridge's `buildRuns`: drop
/// `text`, and drop null/undefined/false option values.
fn bridge_runs(v: Option<&Value>) -> Vec<Run> {
    let Some(Value::Array(items)) = v else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in items {
        let Some(o) = item.as_object() else { continue };
        let text = o.get("text").map(js_to_string).unwrap_or_default();
        let mut opts = Opts::new();
        for (k, v) in o {
            if k == "text" || v.is_null() || v == &Value::Bool(false) {
                continue;
            }
            opts.insert(k.clone(), v.clone());
        }
        out.push(Run {
            text,
            options: opts,
        });
    }
    out
}

fn first_string(a: Option<&Value>, b: Option<&Value>, d: &str) -> Value {
    if truthy(a) {
        return a.unwrap().clone();
    }
    if truthy(b) {
        return b.unwrap().clone();
    }
    Value::String(d.into())
}

impl Deck {
    fn clean_opts(
        &self,
        mut opts: Opts,
        shape_color: Option<&Value>,
        placeholders: &[PlaceholderInfo],
        text_count: usize,
        is_placeholder: bool,
    ) -> Opts {
        // STEP 1 / A.1: color (placeholders inherit instead of defaulting).
        if !truthy(opts.get("placeholder")) && !truthy(opts.get("color")) {
            let color = first_string(opts.get("color"), shape_color, DEF_FONT_COLOR);
            opts.insert("color".into(), color);
        }
        // A.2: placeholders never inherit bullets implicitly.
        if (truthy(opts.get("placeholder")) || is_placeholder) && !truthy(opts.get("bullet")) {
            opts.insert("bullet".into(), Value::Bool(false));
        }
        // A.3: a text box filling a placeholder inherits that placeholder's
        // options (position, margins, font, ph metadata, object name).
        if truthy(opts.get("placeholder"))
            && let Some(name) = opts.get("placeholder").cloned()
            && let Some(info) = placeholders
                .iter()
                .find(|p| p.options.get("placeholder") == Some(&name))
        {
            let mut merged = opts.clone();
            for (k, v) in &info.options {
                merged.insert(k.clone(), v.clone());
            }
            opts = merged;
        }
        // A.4: object name.
        if truthy(opts.get("objectName")) {
            let name = encode_xml(opts.get("objectName").unwrap());
            opts.insert("objectName".into(), Value::String(name));
        } else {
            opts.insert(
                "objectName".into(),
                Value::String(format!("Text {text_count}")),
            );
        }
        // B: line defaults for line shapes.
        if opts.get("shape").and_then(Value::as_str) == Some("line") {
            let line = opts.get("line").and_then(Value::as_object);
            let new_line = json!({
                "type": line.and_then(|l| l.get("type")).filter(|v| truthy(Some(v))).cloned().unwrap_or_else(|| json!("solid")),
                "color": line.and_then(|l| l.get("color")).filter(|v| truthy(Some(v))).cloned().unwrap_or_else(|| json!(DEF_SHAPE_LINE_COLOR)),
                "transparency": line.and_then(|l| l.get("transparency")).cloned().unwrap_or_else(|| json!(0)),
                "width": line.and_then(|l| l.get("width")).filter(|v| truthy(Some(v))).cloned().unwrap_or_else(|| json!(1)),
                "dashType": line.and_then(|l| l.get("dashType")).filter(|v| truthy(Some(v))).cloned().unwrap_or_else(|| json!("solid")),
                "beginArrowType": line.and_then(|l| l.get("beginArrowType")).cloned().unwrap_or(Value::Null),
                "endArrowType": line.and_then(|l| l.get("endArrowType")).cloned().unwrap_or(Value::Null),
            });
            opts.insert("line".into(), new_line);
            for (dep, target) in [
                ("lineSize", "width"),
                ("lineDash", "dashType"),
                ("lineHead", "beginArrowType"),
                ("lineTail", "endArrowType"),
            ] {
                if let Some(v) = opts.get(dep).cloned()
                    && let Some(l) = opts.get_mut("line").and_then(Value::as_object_mut)
                {
                    l.insert(target.into(), v);
                }
            }
        }
        // C: line defaults.
        if !truthy(opts.get("line")) {
            opts.insert("line".into(), json!({}));
        }
        // lineSpacing must be numeric or null.
        for k in ["lineSpacing", "lineSpacingMultiple"] {
            let keep = opts.get(k).filter(|v| truthy(Some(v))).and_then(|v| {
                let n = js_number(v);
                if n.is_nan() { None } else { Some(n) }
            });
            opts.insert(k.into(), keep.map(Value::from).unwrap_or(Value::Null));
        }
        // D: body properties.
        let mut body = opts
            .get("_bodyProp")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        body.insert("autoFit".into(), Value::Bool(opt_bool(&opts, "autoFit")));
        body.insert(
            "anchor".into(),
            if truthy(opts.get("placeholder")) {
                Value::Null
            } else {
                Value::String("ctr".into())
            },
        );
        body.insert(
            "vert".into(),
            opts.get("vert").cloned().unwrap_or(Value::Null),
        );
        body.insert(
            "wrap".into(),
            Value::Bool(match opts.get("wrap") {
                Some(Value::Bool(b)) => *b,
                _ => true,
            }),
        );
        // E: deprecated `inset`.
        if let Some(inset) = opts.get("inset")
            && (js_number(inset) == 0.0 || !js_number(inset).is_nan())
        {
            let v = inch2emu(inset);
            for k in ["lIns", "rIns", "tIns", "bIns"] {
                body.insert(k.into(), Value::from(v));
            }
        }
        // F: deprecated boolean underline.
        if opts.get("underline") == Some(&Value::Bool(true)) {
            opts.insert("underline".into(), json!({"style": "sng"}));
        }
        // STEP 2: align/valign -> bodyProp.
        if let Some(a) = opts.get("align").and_then(Value::as_str) {
            let a = a.to_ascii_lowercase();
            if a.starts_with('c') {
                body.insert("align".into(), json!("center"));
            } else if a.starts_with('l') {
                body.insert("align".into(), json!("left"));
            } else if a.starts_with('r') {
                body.insert("align".into(), json!("right"));
            } else if a.starts_with('j') {
                body.insert("align".into(), json!("justify"));
            }
        }
        if let Some(a) = opts.get("valign").and_then(Value::as_str) {
            let a = a.to_ascii_lowercase();
            if a.starts_with('b') {
                body.insert("anchor".into(), json!("b"));
            } else if a.starts_with('m') {
                body.insert("anchor".into(), json!("ctr"));
            } else if a.starts_with('t') {
                body.insert("anchor".into(), json!("t"));
            }
        }
        opts.insert("_bodyProp".into(), Value::Object(body));
        opts
    }

    fn add_text_definition(
        &mut self,
        target: &mut Target,
        placeholders: Vec<PlaceholderInfo>,
        runs: Vec<Run>,
        options: Opts,
        is_placeholder: bool,
    ) {
        let text_count = target
            .objects
            .iter()
            .filter(|o| o.kind == ObjKind::Text)
            .count();
        let shape_color = options.get("color").cloned();
        let mut shape_opts = self.clean_opts(
            options,
            shape_color.as_ref(),
            &placeholders,
            text_count,
            is_placeholder,
        );
        let shape = shape_opts
            .get("shape")
            .and_then(Value::as_str)
            .map(String::from)
            .unwrap_or_else(|| "rect".into());
        let mut text = if runs.is_empty() {
            vec![Run {
                text: String::new(),
                options: Opts::new(),
            }]
        } else {
            runs
        };
        // STEP 2: clean every run, inheriting from the shape options.
        let run_text_count = target
            .objects
            .iter()
            .filter(|o| o.kind == ObjKind::Text)
            .count();
        for run in &mut text {
            let opts = std::mem::take(&mut run.options);
            run.options = self.clean_opts(
                opts,
                shape_opts.get("color"),
                &placeholders,
                run_text_count,
                is_placeholder,
            );
        }
        create_text_hyperlink_rels(target, &mut text);
        target.objects.push(Obj {
            kind: if is_placeholder {
                ObjKind::Placeholder
            } else {
                ObjKind::Text
            },
            options: std::mem::take(&mut shape_opts),
            text: Some(text),
            shape,
            image: None,
            image_rid: None,
            hyperlink: None,
            has_custom_prompt: is_placeholder,
        });
    }

    fn add_shape_definition(&mut self, target: &mut Target, shape: &str, mut options: Opts) {
        if !truthy(options.get("line")) {
            options.insert("line".into(), json!({"type": "none"}));
        }
        let line = options.get("line").cloned().unwrap_or(Value::Null);
        let new_line = json!({
            "type": line.as_object().and_then(|l| l.get("type")).filter(|v| truthy(Some(v))).cloned().unwrap_or_else(|| json!("solid")),
            "color": line.as_object().and_then(|l| l.get("color")).filter(|v| truthy(Some(v))).cloned().unwrap_or_else(|| json!(DEF_SHAPE_LINE_COLOR)),
            "transparency": line.as_object().and_then(|l| l.get("transparency")).cloned().unwrap_or_else(|| json!(0)),
            "width": line.as_object().and_then(|l| l.get("width")).filter(|v| truthy(Some(v))).cloned().unwrap_or_else(|| json!(1)),
            "dashType": line.as_object().and_then(|l| l.get("dashType")).filter(|v| truthy(Some(v))).cloned().unwrap_or_else(|| json!("solid")),
            "beginArrowType": line.as_object().and_then(|l| l.get("beginArrowType")).cloned().unwrap_or(Value::Null),
            "endArrowType": line.as_object().and_then(|l| l.get("endArrowType")).cloned().unwrap_or(Value::Null),
        });
        if line
            .as_object()
            .and_then(|l| l.get("type"))
            .and_then(Value::as_str)
            != Some("none")
        {
            options.insert("line".into(), new_line);
        }
        for (k, default) in [("x", 1.0), ("y", 1.0), ("w", 1.0), ("h", 1.0)] {
            let explicit_zero = options.get(k).is_some_and(|v| js_number(v) == 0.0);
            if !truthy(options.get(k)) && !explicit_zero {
                options.insert(k.into(), json!(default));
            }
        }
        let count = target
            .objects
            .iter()
            .filter(|o| o.kind == ObjKind::Text)
            .count();
        if truthy(options.get("objectName")) {
            let name = encode_xml(options.get("objectName").unwrap());
            options.insert("objectName".into(), Value::String(name));
        } else {
            options.insert("objectName".into(), Value::String(format!("Shape {count}")));
        }
        create_object_hyperlink_rel(target, &mut options);
        target.objects.push(Obj {
            kind: ObjKind::Text,
            options,
            text: None,
            shape: shape.into(),
            image: None,
            image_rid: None,
            hyperlink: None,
            has_custom_prompt: false,
        });
    }

    fn add_image_definition(
        &mut self,
        target: &mut Target,
        opt: &Opts,
        data: Option<&str>,
        preview: Option<&str>,
    ) {
        let data = data.unwrap_or("");
        let path = opt
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let image_rel_id = get_new_rel_id(target);
        let count = target
            .objects
            .iter()
            .filter(|o| o.kind == ObjKind::Image)
            .count();
        let object_name = if truthy(opt.get("objectName")) {
            encode_xml(opt.get("objectName").unwrap())
        } else {
            format!("Image {count}")
        };
        let mut extn = if !path.is_empty() {
            let tail = path.rsplit('/').next().unwrap_or(&path);
            let tail = tail.split('?').next().unwrap_or(tail);
            tail.rsplit('.')
                .next()
                .unwrap_or("png")
                .split('#')
                .next()
                .unwrap_or("png")
        } else {
            "png"
        }
        .to_ascii_lowercase();
        if !data.is_empty() {
            // `/image\/(\w+);/` first match
            if let Some(rest) = data.split("image/").nth(1)
                && let Some(mime) = rest.split(';').next()
                && !mime.is_empty()
                && mime.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                extn = mime.to_string();
            } else if data.to_ascii_lowercase().contains("image/svg+xml") {
                extn = "svg".into();
            }
        }
        let image = if path.is_empty() {
            "preencoded.png".to_string()
        } else {
            path.clone()
        };
        let preview = self
            .svg_previews
            .get(data)
            .cloned()
            .or_else(|| preview.map(String::from));
        let slide_num = target.slide_num;
        let mut options = Opts::new();
        options.insert(
            "x".into(),
            opt.get("x")
                .cloned()
                .filter(|v| truthy(Some(v)))
                .unwrap_or_else(|| json!(0)),
        );
        options.insert(
            "y".into(),
            opt.get("y")
                .cloned()
                .filter(|v| truthy(Some(v)))
                .unwrap_or_else(|| json!(0)),
        );
        options.insert(
            "w".into(),
            opt.get("w")
                .cloned()
                .filter(|v| truthy(Some(v)))
                .unwrap_or_else(|| json!(1)),
        );
        options.insert(
            "h".into(),
            opt.get("h")
                .cloned()
                .filter(|v| truthy(Some(v)))
                .unwrap_or_else(|| json!(1)),
        );
        options.insert(
            "altText".into(),
            opt.get("altText")
                .cloned()
                .filter(|v| truthy(Some(v)))
                .unwrap_or_else(|| json!("")),
        );
        options.insert(
            "rounding".into(),
            opt.get("rounding")
                .cloned()
                .filter(|v| matches!(v, Value::Bool(_)))
                .unwrap_or(json!(false)),
        );
        if let Some(v) = opt.get("sizing") {
            options.insert("sizing".into(), v.clone());
        }
        if let Some(v) = opt.get("placeholder") {
            options.insert("placeholder".into(), v.clone());
        }
        options.insert(
            "rotate".into(),
            opt.get("rotate")
                .cloned()
                .filter(|v| truthy(Some(v)))
                .unwrap_or_else(|| json!(0)),
        );
        options.insert(
            "flipV".into(),
            opt.get("flipV")
                .cloned()
                .filter(|v| truthy(Some(v)))
                .unwrap_or(json!(false)),
        );
        options.insert(
            "flipH".into(),
            opt.get("flipH")
                .cloned()
                .filter(|v| truthy(Some(v)))
                .unwrap_or(json!(false)),
        );
        options.insert(
            "transparency".into(),
            opt.get("transparency")
                .cloned()
                .filter(|v| truthy(Some(v)))
                .unwrap_or_else(|| json!(0)),
        );
        options.insert("objectName".into(), Value::String(object_name));
        if let Some(v) = opt.get("shadow") {
            options.insert("shadow".into(), v.clone());
        }

        let mut hyperlink = None;
        if extn == "svg" {
            let png_target = format!(
                "../media/image-{}-{}.png",
                slide_num
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "null".into()),
                target.media.len() + 1
            );
            target.media.push(MediaRel {
                r_id: image_rel_id,
                type_: "image/png".into(),
                extn: "png".into(),
                data_uri: preview.clone().or_else(data_uri_fallback),
                target: png_target,
            });
            let svg_target = format!(
                "../media/image-{}-{}.{}",
                slide_num
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "null".into()),
                target.media.len() + 1,
                extn
            );
            target.media.push(MediaRel {
                r_id: image_rel_id + 1,
                type_: "image/svg+xml".into(),
                extn: extn.clone(),
                data_uri: if data.is_empty() {
                    None
                } else {
                    Some(data.to_string())
                },
                target: svg_target,
            });
        } else {
            let target_name = format!(
                "../media/image-{}-{}.{}",
                slide_num
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "null".into()),
                target.media.len() + 1,
                extn
            );
            target.media.push(MediaRel {
                r_id: image_rel_id,
                type_: format!("image/{extn}"),
                extn: extn.clone(),
                data_uri: if data.is_empty() {
                    None
                } else {
                    Some(data.to_string())
                },
                target: target_name,
            });
        }
        let image_rid = if extn == "svg" {
            image_rel_id + 1
        } else {
            image_rel_id
        };
        if let Some(h) = opt.get("hyperlink").and_then(Value::as_object) {
            let mut h = h.clone();
            if h.get("url").is_none() && h.get("slide").is_none() {
                // Match pptxgenjs's throw closely enough to fail the build.
                return;
            }
            let r_id = image_rid + 1;
            target.rels.push(Rel {
                r_id,
                kind: "hyperlink".into(),
                data: Some(if h.contains_key("slide") {
                    "slide".into()
                } else {
                    "dummy".into()
                }),
                target: encode_xml(h.get("url").or_else(|| h.get("slide")).unwrap()),
            });
            h.insert("_rId".into(), json!(r_id));
            hyperlink = Some(h);
        }
        target.objects.push(Obj {
            kind: ObjKind::Image,
            options,
            text: None,
            shape: "rect".into(),
            image: Some(image),
            image_rid: Some(image_rid),
            hyperlink,
            has_custom_prompt: false,
        });
    }

    fn add_notes_definition(target: &mut Target, notes: String) {
        target.objects.push(Obj {
            kind: ObjKind::Notes,
            options: Opts::new(),
            text: Some(vec![Run {
                text: notes,
                options: Opts::new(),
            }]),
            shape: "rect".into(),
            image: None,
            image_rid: None,
            hyperlink: None,
            has_custom_prompt: false,
        });
    }
}

fn data_uri_fallback() -> Option<String> {
    Some(
        "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII="
            .into(),
    )
}

fn create_text_hyperlink_rels(target: &mut Target, text: &mut [Run]) {
    for run in text {
        let Some(h) = run.options.get("hyperlink").and_then(Value::as_object) else {
            continue;
        };
        if h.get("_rId").is_some() {
            continue;
        }
        let mut h = h.clone();
        if h.get("url").is_none() && h.get("slide").is_none() {
            continue;
        }
        let r_id = get_new_rel_id(target);
        target.rels.push(Rel {
            r_id,
            kind: "hyperlink".into(),
            data: Some(if h.contains_key("slide") {
                "slide".into()
            } else {
                "dummy".into()
            }),
            target: encode_xml(h.get("url").or_else(|| h.get("slide")).unwrap()),
        });
        h.insert("_rId".into(), json!(r_id));
        run.options.insert("hyperlink".into(), Value::Object(h));
    }
}

fn create_object_hyperlink_rel(target: &mut Target, options: &mut Opts) {
    let Some(h) = options.get("hyperlink").and_then(Value::as_object) else {
        return;
    };
    if h.get("_rId").is_some() {
        return;
    }
    if h.get("url").is_none() && h.get("slide").is_none() {
        return;
    }
    let mut h = h.clone();
    let r_id = get_new_rel_id(target);
    target.rels.push(Rel {
        r_id,
        kind: "hyperlink".into(),
        data: Some(if h.contains_key("slide") {
            "slide".into()
        } else {
            "dummy".into()
        }),
        target: encode_xml(h.get("url").or_else(|| h.get("slide")).unwrap()),
    });
    h.insert("_rId".into(), json!(r_id));
    options.insert("hyperlink".into(), Value::Object(h));
}

fn collect_svg_previews(spec: &Value) -> std::collections::HashMap<String, String> {
    let mut previews = std::collections::HashMap::new();
    if let Some(masters) = spec.get("masters").and_then(Value::as_array) {
        for object in masters
            .iter()
            .filter_map(|m| m.get("objects"))
            .filter_map(Value::as_array)
            .flatten()
        {
            if let (Some(data), Some(preview)) = (
                object.get("data").and_then(Value::as_str),
                object.get("preview").and_then(Value::as_str),
            ) {
                previews.insert(data.to_string(), preview.to_string());
            }
        }
    }
    if let Some(sections) = spec.get("sections").and_then(Value::as_array) {
        for shape in sections
            .iter()
            .filter_map(|s| s.get("slides"))
            .filter_map(Value::as_array)
            .flatten()
            .filter_map(|s| s.get("shapes"))
            .filter_map(Value::as_array)
            .flatten()
        {
            if let (Some(data), Some(preview)) = (
                shape.get("data").and_then(Value::as_str),
                shape.get("preview").and_then(Value::as_str),
            ) {
                previews.insert(data.to_string(), preview.to_string());
            }
        }
    }
    previews
}

/// Build a deck from the pptxgenjs-shaped JSON spec.
fn build_deck(spec_json: &str) -> Result<Deck> {
    let spec: Value =
        serde_json::from_str(spec_json).map_err(|e| miette!("pptx spec is not valid JSON: {e}"))?;
    let width_in = spec.get("width").and_then(Value::as_f64).unwrap_or(10.0);
    let height_in = spec.get("height").and_then(Value::as_f64).unwrap_or(7.5);
    let width = js_round(width_in * EMU);
    let height = js_round(height_in * EMU);
    let major_font = spec
        .get("majorFont")
        .and_then(Value::as_str)
        .unwrap_or("Calibri Light")
        .to_string();
    let minor_font = spec
        .get("minorFont")
        .and_then(Value::as_str)
        .unwrap_or("Calibri")
        .to_string();

    let mut deck = Deck {
        width,
        height,
        major_font,
        minor_font,
        layouts: Vec::new(),
        master: Target::default(),
        slides: Vec::new(),
        sections: Vec::new(),
        section_uuids: Vec::new(),
        svg_previews: collect_svg_previews(&spec),
    };

    // Masters: pptxgenjs's defineSlideMaster.
    let masters = spec
        .get("masters")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for (mi, m) in masters.iter().enumerate() {
        let Some(m) = m.as_object() else { continue };
        let title = m
            .get("name")
            .map(js_to_string)
            .unwrap_or_else(|| format!("master{mi}"));
        let slide_num = 1000 + (deck.layouts.len() as u64 + 1);
        let mut layout = Target {
            name: Some(title),
            slide_num: Some(slide_num),
            ..Target::default()
        };
        if let Some(bg) = m.get("background").and_then(Value::as_object) {
            let mut bg = bg.clone();
            if truthy(bg.get("fill")) {
                bg.insert("color".into(), bg["fill"].clone());
            }
            layout.background = Some(bg);
        }
        let objects = m
            .get("objects")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for (idx, o) in objects.iter().enumerate() {
            let Some(o) = o.as_object() else { continue };
            let ty = o.get("type").and_then(Value::as_str).unwrap_or("");
            let mut opts = o
                .get("options")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            for k in ["x", "y", "w", "h"] {
                if let Some(v) = o.get(k) {
                    opts.insert(k.into(), v.clone());
                }
            }
            let data = o.get("data").and_then(Value::as_str);
            let preview = o.get("preview").and_then(Value::as_str);
            if ty == "text" || ty == "placeholder" {
                let runs = bridge_runs(o.get("runs"));
                let runs = if runs.is_empty() && o.get("text").is_some() {
                    vec![Run {
                        text: o.get("text").map(js_to_string).unwrap_or_default(),
                        options: Opts::new(),
                    }]
                } else if runs.is_empty() {
                    // The bridge passed an empty run array to addTextDefinition;
                    // pptxgenjs treats that as one empty run.
                    vec![Run {
                        text: String::new(),
                        options: Opts::new(),
                    }]
                } else {
                    runs
                };
                if ty == "placeholder" {
                    if let Some(name) = opts.remove("name") {
                        opts.insert("placeholder".into(), name);
                    }
                    if let Some(t) = opts.remove("type") {
                        opts.insert("_placeholderType".into(), t);
                    }
                    opts.insert("_placeholderIdx".into(), json!(100 + idx));
                    deck.add_text_definition(&mut layout, Vec::new(), runs, opts, true);
                } else {
                    deck.add_text_definition(&mut layout, Vec::new(), runs, opts, false);
                }
            } else if ty == "image" {
                if let Some(d) = data {
                    opts.insert("data".into(), json!(d));
                }
                deck.add_image_definition(&mut layout, &opts, data, preview);
            } else if ty == "rect" || ty == "line" {
                let shape = if ty == "rect" { "rect" } else { "line" };
                deck.add_shape_definition(&mut layout, shape, opts);
            }
        }
        let sn = m.get("slideNumber").and_then(Value::as_object).cloned();
        if let Some(sn) = &sn {
            layout.slide_number = Some(sn.clone());
            if deck.master.slide_number.is_none() {
                deck.master.slide_number = Some(sn.clone());
            }
        }
        deck.layouts.push(layout);
    }

    // Slides: addSection/addSlide + renderSlide from the bridge.
    let sections = spec
        .get("sections")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut lcg = Lcg::new();
    // The legacy runtime consumed one Math.random value before the first
    // section UUID; keep the exact deterministic sequence.
    lcg.next();
    for sec in sections {
        let Some(sec) = sec.as_object() else { continue };
        let title = sec.get("title").map(js_to_string).unwrap_or_default();
        let mut section = Section {
            title: title.clone(),
            slide_ids: Vec::new(),
        };
        let slides = sec
            .get("slides")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for s in slides {
            let Some(s) = s.as_object() else { continue };
            let n = deck.slides.len() + 1;
            let master = s
                .get("master")
                .and_then(Value::as_str)
                .ok_or_else(|| miette!("slide {n}: spec is missing `master`"))?;
            let layout_idx = deck
                .layout_index(master)
                .ok_or_else(|| miette!("slide {n}: unknown master `{master}`"))?;
            let mut slide = Target {
                name: Some(format!("Slide {n}")),
                slide_num: Some(n as u64),
                slide_layout: Some(layout_idx),
                slide_number: deck.layouts[layout_idx].slide_number.clone(),
                ..Target::default()
            };
            if let Some(bg) = s.get("background").and_then(Value::as_object) {
                let mut bg = bg.clone();
                if truthy(bg.get("fill")) {
                    bg.insert("color".into(), bg["fill"].clone());
                }
                slide.background = Some(bg);
            }
            if truthy(s.get("notes")) {
                Deck::add_notes_definition(&mut slide, s.get("notes").map(js_to_string).unwrap());
            }
            let placeholders = deck.layout_placeholders(layout_idx);
            let shapes = s
                .get("shapes")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for sh in shapes {
                let Some(sh) = sh.as_object() else { continue };
                let kind = sh.get("kind").and_then(Value::as_str).unwrap_or("");
                let mut opts = Opts::new();
                for (k, v) in sh {
                    if matches!(k.as_str(), "kind" | "runs" | "type" | "preview") || v.is_null() {
                        continue;
                    }
                    opts.insert(k.clone(), v.clone());
                }
                match kind {
                    "text" => {
                        let runs = bridge_runs(sh.get("runs"));
                        deck.add_text_definition(
                            &mut slide,
                            placeholders.clone(),
                            runs,
                            opts,
                            false,
                        );
                    }
                    "shape" => {
                        let ty = sh.get("type").and_then(Value::as_str).unwrap_or("rect");
                        deck.add_shape_definition(&mut slide, ty, opts);
                    }
                    "image" => {
                        let data = sh.get("data").and_then(Value::as_str);
                        let preview = sh.get("preview").and_then(Value::as_str);
                        deck.add_image_definition(&mut slide, &opts, data, preview);
                    }
                    _ => {}
                }
            }
            deck.slides.push(slide);
            section.slide_ids.push(n - 1);
        }
        deck.sections.push(section);
    }

    // exportPresentation: append a placeholder for every layout placeholder the
    // slide did not already fill.
    for si in 0..deck.slides.len() {
        let layout_idx = deck.slides[si].slide_layout.unwrap_or(0);
        let placeholders = deck.layout_placeholders(layout_idx);
        let existing: HashSet<String> = deck.slides[si]
            .objects
            .iter()
            .filter_map(|o| o.options.get("placeholder").map(js_to_string))
            .collect();
        let mut slide = deck.slides[si].clone();
        for ph in placeholders {
            let Some(name) = ph.options.get("placeholder").map(js_to_string) else {
                continue;
            };
            if existing.contains(&name) {
                continue;
            }
            let mut opts = ph.options.clone();
            // addPlaceholdersToSlideLayouts passes the layout options object
            // itself, so the placeholder flag stays set.
            opts.insert("placeholder".into(), Value::String(name));
            let phs = deck.layout_placeholders(layout_idx);
            deck.add_text_definition(&mut slide, phs, Vec::new(), opts, false);
        }
        deck.slides[si] = slide;
    }

    deck.section_uuids = lcg_uuids(&mut lcg, deck.sections.len());
    Ok(deck)
}

fn lcg_uuids(lcg: &mut Lcg, n: usize) -> Vec<String> {
    (0..n).map(|_| lcg.uuid()).collect()
}

/// `build_deck` is split from packaging because `Deck` contains the section
/// UUIDs as a transient field.
impl Deck {
    pub fn render(&self) -> Vec<u8> {
        package(self)
    }
}

/// Render the spec directly to `.pptx` bytes.
pub fn render(spec_json: &str) -> Result<Vec<u8>> {
    let deck = build_deck(spec_json)?;
    Ok(deck.render())
}

/// Default theme used by the preview pipeline for these fonts.
pub(crate) fn default_theme_xml(major_font: &str, minor_font: &str) -> String {
    emit::theme_xml(major_font, minor_font)
}

/// The master's default text styles.  The preview pipeline only parses
/// `titleStyle`/`bodyStyle`/`otherStyle` from this fragment.
pub(crate) fn default_master_styles_xml() -> String {
    format!(
        "<p:sldMaster xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\">{}",
        String::from_utf8_lossy(include_bytes!("static/master_tail.xml"))
    )
}

fn package(deck: &Deck) -> Vec<u8> {
    let mut entries: Vec<Entry> = Vec::new();
    let mut seen = HashSet::new();
    let mut push = |name: String, data: Vec<u8>, dir: bool, seen: &mut HashSet<String>| {
        if seen.insert(name.clone()) {
            entries.push(Entry { name, data, dir });
        }
    };

    for dir in [
        "_rels",
        "docProps",
        "ppt",
        "ppt/charts",
        "ppt/embeddings",
        "ppt/media",
        "ppt/slideLayouts",
        "ppt/slideMasters",
        "ppt/slides",
        "ppt/theme",
        "ppt/notesMasters",
        "ppt/notesSlides",
    ] {
        push(dir.into(), Vec::new(), true, &mut seen);
    }

    push(
        "[Content_Types].xml".into(),
        make_xml_content_types(deck).into_bytes(),
        false,
        &mut seen,
    );
    push(
        "_rels/.rels".into(),
        include_bytes!("static/root_rels.xml").to_vec(),
        false,
        &mut seen,
    );
    push(
        "docProps/app.xml".into(),
        make_xml_app(deck).into_bytes(),
        false,
        &mut seen,
    );
    push(
        "docProps/core.xml".into(),
        make_xml_core().into_bytes(),
        false,
        &mut seen,
    );
    push(
        "ppt/_rels/presentation.xml.rels".into(),
        make_xml_presentation_rels(deck).into_bytes(),
        false,
        &mut seen,
    );
    push(
        "ppt/theme/theme1.xml".into(),
        make_xml_theme(deck).into_bytes(),
        false,
        &mut seen,
    );
    push(
        "ppt/presentation.xml".into(),
        make_xml_presentation(deck).into_bytes(),
        false,
        &mut seen,
    );
    push(
        "ppt/presProps.xml".into(),
        include_bytes!("static/pres_props.xml").to_vec(),
        false,
        &mut seen,
    );
    push(
        "ppt/tableStyles.xml".into(),
        include_bytes!("static/table_styles.xml").to_vec(),
        false,
        &mut seen,
    );
    push(
        "ppt/viewProps.xml".into(),
        include_bytes!("static/view_props.xml").to_vec(),
        false,
        &mut seen,
    );

    for (i, layout) in deck.layouts.iter().enumerate() {
        push(
            format!("ppt/slideLayouts/slideLayout{}.xml", i + 1),
            make_xml_layout(layout, deck, None).into_bytes(),
            false,
            &mut seen,
        );
        push(
            format!("ppt/slideLayouts/_rels/slideLayout{}.xml.rels", i + 1),
            make_xml_layout_rel(layout).into_bytes(),
            false,
            &mut seen,
        );
    }

    let lock_parts = lock_targets(deck);
    for (i, slide) in deck.slides.iter().enumerate() {
        let layout_idx = slide.slide_layout.unwrap_or(0);
        let phs = deck.layout_placeholders(layout_idx);
        let mut xml = make_xml_slide(slide, deck, &phs);
        if lock_parts.contains(&format!("ppt/slides/slide{}.xml", i + 1)) {
            xml = lock_slide_numbers(&xml);
        }
        push(
            format!("ppt/slides/slide{}.xml", i + 1),
            xml.into_bytes(),
            false,
            &mut seen,
        );
        push(
            format!("ppt/slides/_rels/slide{}.xml.rels", i + 1),
            make_xml_slide_rel(slide, deck, i + 1).into_bytes(),
            false,
            &mut seen,
        );
        push(
            format!("ppt/notesSlides/notesSlide{}.xml", i + 1),
            make_xml_notes_slide(slide).into_bytes(),
            false,
            &mut seen,
        );
        push(
            format!("ppt/notesSlides/_rels/notesSlide{}.xml.rels", i + 1),
            make_xml_notes_slide_rel(i + 1).into_bytes(),
            false,
            &mut seen,
        );
    }

    let mut master_xml = finish_master(make_xml_master(&deck.master, deck)).replace(
        "{{LAYOUT_IDS}}",
        &master_layout_ids(&deck.master, &deck.layouts),
    );
    master_xml = strip_master_slide_number(&master_xml);
    push(
        "ppt/slideMasters/slideMaster1.xml".into(),
        master_xml.into_bytes(),
        false,
        &mut seen,
    );
    push(
        "ppt/slideMasters/_rels/slideMaster1.xml.rels".into(),
        make_xml_master_rel(&deck.master, &deck.layouts).into_bytes(),
        false,
        &mut seen,
    );
    push(
        "ppt/notesMasters/notesMaster1.xml".into(),
        include_bytes!("static/notes_master.xml").to_vec(),
        false,
        &mut seen,
    );
    push(
        "ppt/notesMasters/_rels/notesMaster1.xml.rels".into(),
        include_bytes!("static/notes_master_rels.xml").to_vec(),
        false,
        &mut seen,
    );

    // Media: layouts, then slides, then the shared master. First occurrence of
    // a target wins (the legacy capture deduped names before zipping).
    for t in deck
        .layouts
        .iter()
        .chain(deck.slides.iter())
        .chain(std::iter::once(&deck.master))
    {
        for rel in &t.media {
            let name = rel.target.replace("..", "ppt");
            let data = rel
                .data_uri
                .as_deref()
                .and_then(|d| d.split(',').next_back())
                .unwrap_or("");
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(data.trim())
                .unwrap_or_default();
            push(name, bytes, false, &mut seen);
        }
    }

    zip::write(&entries)
}

fn lock_targets(deck: &Deck) -> HashSet<String> {
    let mut out = HashSet::new();
    let locked: Vec<bool> = deck
        .layouts
        .iter()
        .map(|l| {
            l.slide_number
                .as_ref()
                .and_then(|s| s.get("locked"))
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .collect();
    for (i, slide) in deck.slides.iter().enumerate() {
        let idx = slide.slide_layout.unwrap_or(0);
        if locked.get(idx).copied().unwrap_or(false) {
            out.insert(format!("ppt/slides/slide{}.xml", i + 1));
        }
    }
    out
}

fn strip_master_slide_number(xml: &str) -> String {
    let anchor = "name=\"Slide Number Placeholder 0\"";
    let Some(at) = xml.find(anchor) else {
        return xml.to_string();
    };
    let Some(sp) = xml[..at].rfind("<p:sp>") else {
        return xml.to_string();
    };
    let Some(rel) = xml[sp..].find("</p:sp>") else {
        return xml.to_string();
    };
    let end = sp + rel + "</p:sp>".len();
    format!("{}{}", &xml[..sp], &xml[end..])
}

fn lock_slide_numbers(xml: &str) -> String {
    let mut out = xml.to_string();
    let mut pos = 0;
    while let Some(ph_rel) = out[pos..].find("<p:ph type=\"sldNum\"") {
        let ph_start = pos + ph_rel;
        let Some(ph_end_rel) = out[ph_start..].find("/>") else {
            break;
        };
        let ph_end = ph_start + ph_end_rel + 2;
        let Some(nv_start) = out[..ph_start].rfind("<p:nvSpPr>") else {
            break;
        };
        let Some(nv_rel) = out[nv_start..].find("</p:nvSpPr>") else {
            break;
        };
        let nv_end = nv_start + nv_rel;
        let mut nvsp = out[nv_start..nv_end].to_string();
        nvsp.replace_range(ph_start - nv_start..ph_end - nv_start, "");
        nvsp = nvsp.replace(
            "<a:spLocks noGrp=\"1\"/>",
            "<a:spLocks noGrp=\"1\" noSelect=\"1\" noMove=\"1\" noResize=\"1\" noTextEdit=\"1\"/>",
        );
        out.replace_range(nv_start..nv_end, &nvsp);
        let after = nv_start + nvsp.len();
        let Some(close_rel) = out[after..].find("</p:sp>") else {
            break;
        };
        pos = after + close_rel + "</p:sp>".len();
    }
    out
}
