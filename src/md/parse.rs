use pulldown_cmark::{Event, Options, Parser as MdParser, Tag, TagEnd};
use std::collections::HashMap;

use serde_json::{Map, Value, json};

/// An inline-level token extracted from the event stream.
#[derive(Debug, Clone)]
enum Inl {
    Text(String),
    Bold(bool),
    Italic(bool),
    /// An opening `<span data-*>` carrying its font attributes, `Some(empty)`
    /// for a bare `<span>` run separator.
    SpanOpen(Map<String, Value>),
    SpanClose,
    /// A `<br>` inside a table cell: separates paragraphs.
    Br,
}

impl Inl {
    fn from_html(tag: &str) -> Inl {
        if tag == "<br>" || tag == "<br/>" || tag == "<br />" {
            return Inl::Br;
        }
        if tag.starts_with("</") {
            return Inl::SpanClose;
        }
        Inl::SpanOpen(span_attrs(tag))
    }
}

/// Assembles inline events into a sequence of run objects.
struct Runs {
    runs: Vec<Value>,
    cur: Option<Map<String, Value>>,
    bold: u32,
    italic: u32,
    span: Option<Map<String, Value>>,
    span_active: bool,
}

impl Runs {
    fn new() -> Self {
        Runs {
            runs: Vec::new(),
            cur: None,
            bold: 0,
            italic: 0,
            span: None,
            span_active: false,
        }
    }

    fn text(&mut self, t: &str) {
        if t.is_empty() && self.cur.is_none() {
            return;
        }
        if self.cur.is_none()
            && let Some(font) = self.span.clone()
        {
            let mut run = Map::new();
            run.insert("text".into(), json!(""));
            run.insert("font".into(), Value::Object(font));
            self.cur = Some(run);
        }
        let run = self.cur.get_or_insert_with(Map::new);
        if let Some(Value::String(s)) = run.get_mut("text") {
            s.push_str(t);
            return;
        }
        run.insert("text".into(), json!(t));
    }

    fn bold(&mut self, on: bool) {
        if on {
            self.flush();
            self.bold += 1;
        } else {
            // Flush while the counter is still set so the run is tagged bold.
            self.flush();
            self.bold = self.bold.saturating_sub(1);
        }
    }

    fn italic(&mut self, on: bool) {
        if on {
            self.flush();
            self.italic += 1;
        } else {
            // Flush while the counter is still set so the run is tagged italic.
            self.flush();
            self.italic = self.italic.saturating_sub(1);
        }
    }

    fn span_open(&mut self, attrs: Map<String, Value>) {
        self.flush();
        self.span = if attrs.is_empty() {
            None
        } else {
            Some(font_from_span(&attrs))
        };
        self.span_active = true;
    }

    fn span_close(&mut self) {
        if !self.span_active {
            return;
        }
        if self.cur.is_none()
            && let Some(font) = self.span.take()
        {
            let mut run = Map::new();
            run.insert("text".into(), json!(""));
            run.insert("font".into(), Value::Object(font));
            self.cur = Some(run);
        }
        self.flush();
        self.span = None;
        self.span_active = false;
    }

    fn flush(&mut self) {
        let Some(mut run) = self.cur.take() else {
            return;
        };
        let mut font = match run.remove("font") {
            Some(Value::Object(f)) => f,
            _ => Map::new(),
        };
        if self.bold > 0 {
            font.insert("bold".into(), json!(true));
        }
        if self.italic > 0 {
            font.insert("italic".into(), json!(true));
        }
        if !font.is_empty() {
            run.insert("font".into(), Value::Object(font));
        }
        self.runs.push(Value::Object(run));
    }

    fn finish(&mut self) -> Vec<Value> {
        self.flush();
        std::mem::take(&mut self.runs)
    }
}

fn font_from_span(attrs: &Map<String, Value>) -> Map<String, Value> {
    let mut font = Map::new();
    for (key, field) in [
        ("size", "size"),
        ("name", "name"),
        ("underline", "underline"),
        ("bold", "bold"),
        ("italic", "italic"),
    ] {
        if let Some(v) = attrs.get(key) {
            font.insert(field.to_string(), v.clone());
        }
    }
    if let Some(v) = attrs.get("color").and_then(Value::as_str) {
        font.insert("color".into(), color_from_attr(v));
    }
    font
}

fn color_from_attr(s: &str) -> Value {
    let (ty, value) = s.split_once(':').unwrap_or(("", s));
    let mut color = Map::new();
    if !ty.is_empty() {
        color.insert("type".into(), json!(ty));
    }
    if ty.eq_ignore_ascii_case("RGB") {
        color.insert("rgb".into(), json!(value));
    } else if ty.eq_ignore_ascii_case("SCHEME") {
        color.insert("theme_color".into(), json!(value));
    }
    json!(color)
}

/// Parse `<span data-size=2000 data-color="RGB:FF0000">` into a font map.
fn span_attrs(s: &str) -> Map<String, Value> {
    let inner = s
        .strip_prefix("<span")
        .and_then(|t| t.strip_suffix('>'))
        .unwrap_or("");
    let raw = parse_attrs(inner);
    let mut map = Map::new();
    for (k, v) in raw {
        let key = k.strip_prefix("data-").unwrap_or(&k).to_string();
        let val = match key.as_str() {
            "size" => json!(v.parse::<i64>().unwrap_or(0)),
            "bold" | "italic" | "underline" => json!(v == "true" || v == "1"),
            _ => json!(v),
        };
        map.insert(key, val);
    }
    map
}

struct TfBuf {
    props: Map<String, Value>,
    dps: Option<Value>,
    paragraphs: Vec<Value>,
}

impl Default for TfBuf {
    fn default() -> Self {
        TfBuf {
            props: Map::new(),
            dps: None,
            paragraphs: Vec::new(),
        }
    }
}

struct ShapeBuf {
    shape_type: Option<String>,
    name: Option<String>,
    x: Option<i64>,
    y: Option<i64>,
    w: Option<i64>,
    h: Option<i64>,
    rotation: Option<f64>,
    autoshape: Option<String>,
    fill: Option<Value>,
    outline: Option<Value>,
    crop: Option<Value>,
    tf: Option<TfBuf>,
    table: Option<Value>,
    table_grid: Option<String>,
}

impl ShapeBuf {
    fn new() -> Self {
        ShapeBuf {
            shape_type: None,
            name: None,
            x: None,
            y: None,
            w: None,
            h: None,
            rotation: None,
            autoshape: None,
            fill: None,
            outline: None,
            crop: None,
            tf: None,
            table: None,
            table_grid: None,
        }
    }
}

struct MasterBuf {
    shapes: Vec<Value>,
}

#[derive(Default)]
struct SlideBuf {
    background: Option<String>,
    shapes: Vec<Value>,
    notes_shapes: Option<Vec<Value>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum TableMode {
    CoreProperties,
    ThemeColors,
    ThemeFonts,
    ShapeTable,
}

/// A paragraph under construction.
struct ParaBuf {
    level: i32,
    attrs: HashMap<String, String>,
    runs: Runs,
}

pub struct Parser {
    slide_width: Option<i64>,
    slide_height: Option<i64>,
    core_properties: Vec<(String, String)>,
    theme_colors: Vec<(String, String)>,
    theme_fonts: Vec<(String, String)>,
    masters: Vec<Value>,
    slides: Vec<Value>,

    master: Option<MasterBuf>,
    slide: Option<SlideBuf>,
    in_notes: bool,
    shape: Option<ShapeBuf>,

    pending_dps: bool,
    para_attrs: Option<HashMap<String, String>>,

    table_mode: Option<TableMode>,
    in_table: bool,
    table_rows: Vec<Vec<Value>>,
    table_row: Vec<Value>,
    cell_tokens: Vec<Inl>,

    para: Option<ParaBuf>,
    list_depth: usize,
}

impl Parser {
    pub fn new() -> Self {
        Parser {
            slide_width: None,
            slide_height: None,
            core_properties: Vec::new(),
            theme_colors: Vec::new(),
            theme_fonts: Vec::new(),
            masters: Vec::new(),
            slides: Vec::new(),
            master: None,
            slide: None,
            in_notes: false,
            shape: None,
            pending_dps: false,
            para_attrs: None,
            table_mode: None,
            in_table: false,
            table_rows: Vec::new(),
            table_row: Vec::new(),
            cell_tokens: Vec::new(),
            para: None,
            list_depth: 0,
        }
    }

    fn handle(&mut self, event: Event<'_>) {
        match event {
            Event::Html(t) => self.handle_comment(&t),
            Event::InlineHtml(t) => self.handle_inline(Inl::from_html(&t)),
            Event::Text(t) => self.handle_inline(Inl::Text(t.to_string())),
            Event::Code(t) => self.handle_inline(Inl::Text(t.to_string())),
            Event::SoftBreak | Event::HardBreak => self.handle_inline(Inl::Text("\n".into())),
            Event::Start(Tag::Strong) => self.handle_inline(Inl::Bold(true)),
            Event::End(TagEnd::Strong) => self.handle_inline(Inl::Bold(false)),
            Event::Start(Tag::Emphasis) => self.handle_inline(Inl::Italic(true)),
            Event::End(TagEnd::Emphasis) => self.handle_inline(Inl::Italic(false)),
            Event::Start(Tag::Paragraph) | Event::Start(Tag::Heading { .. }) => {
                self.begin_paragraph()
            }
            Event::End(TagEnd::Paragraph) | Event::End(TagEnd::Heading(_)) => self.end_paragraph(),
            Event::Start(Tag::List(..)) => self.list_depth += 1,
            Event::End(TagEnd::List(_)) => self.list_depth -= 1,
            Event::Start(Tag::Item) => self.begin_paragraph(),
            Event::End(TagEnd::Item) => self.end_paragraph(),
            Event::Start(Tag::Table(..)) => self.begin_table(),
            Event::End(TagEnd::Table) => self.end_table(),
            Event::Start(Tag::TableHead) | Event::Start(Tag::TableRow) => self.table_row.clear(),
            Event::End(TagEnd::TableHead) | Event::End(TagEnd::TableRow) => {
                let cells = std::mem::take(&mut self.table_row);
                self.table_rows.push(cells);
            }
            Event::Start(Tag::TableCell) => self.cell_tokens.clear(),
            Event::End(TagEnd::TableCell) => {
                let tokens = std::mem::take(&mut self.cell_tokens);
                self.table_row.push(cell_value(&tokens));
            }
            _ => {}
        }
    }

    fn finish(mut self) -> Value {
        self.finalize_shape();
        self.finalize_slide();
        self.finalize_master();

        let mut root = Map::new();
        root.insert("slide_width".into(), json!(self.slide_width.unwrap_or(0)));
        root.insert("slide_height".into(), json!(self.slide_height.unwrap_or(0)));
        root.insert("slides".into(), json!(self.slides));
        root.insert("slide_masters".into(), json!(self.masters));
        root.insert(
            "theme".into(),
            json!({
                "colors": pairs_to_map(self.theme_colors),
                "fonts": pairs_to_map(self.theme_fonts),
            }),
        );
        root.insert(
            "core_properties".into(),
            Value::Object(pairs_to_map(self.core_properties)),
        );
        Value::Object(root)
    }

    // -- comments -----------------------------------------------------------

    fn handle_comment(&mut self, text: &str) {
        self.end_paragraph();
        let Some((key, attrs)) = parse_comment(text) else {
            return;
        };
        match key.as_str() {
            "pptx" => {
                self.slide_width = attr_i64(&attrs, "slide_width");
                self.slide_height = attr_i64(&attrs, "slide_height");
            }
            "core_properties" => self.table_mode = Some(TableMode::CoreProperties),
            "theme_colors" => self.table_mode = Some(TableMode::ThemeColors),
            "theme_fonts" => self.table_mode = Some(TableMode::ThemeFonts),
            "master" => {
                self.finalize_shape();
                self.finalize_slide();
                self.finalize_master();
                self.master = Some(MasterBuf { shapes: Vec::new() });
            }
            "slide" => {
                self.finalize_shape();
                self.finalize_slide();
                self.slide = Some(SlideBuf {
                    background: attrs.get("background").cloned(),
                    shapes: Vec::new(),
                    notes_shapes: None,
                });
                self.in_notes = false;
            }
            "notes" => {
                self.finalize_shape();
                self.in_notes = true;
                if let Some(slide) = &mut self.slide {
                    slide.notes_shapes.get_or_insert_with(Vec::new);
                }
            }
            "shape" => {
                self.finalize_shape();
                let mut shape = ShapeBuf::new();
                shape.shape_type = attr_str(&attrs, "type");
                shape.name = attr_str(&attrs, "name");
                shape.x = attr_i64(&attrs, "x");
                shape.y = attr_i64(&attrs, "y");
                shape.w = attr_i64(&attrs, "w");
                shape.h = attr_i64(&attrs, "h");
                shape.rotation = attr_f64(&attrs, "rotation");
                shape.autoshape = attr_str(&attrs, "autoshape");
                shape.fill = attr_json(&attrs, "fill");
                shape.outline = attr_json(&attrs, "outline");
                shape.crop = attr_json(&attrs, "crop");
                self.shape = Some(shape);
            }
            "tf" => {
                self.finalize_paragraph();
                let tf = self
                    .shape
                    .as_mut()
                    .map(|s| s.tf.get_or_insert_with(TfBuf::default));
                if let Some(tf) = tf {
                    apply_tf_props(&mut tf.props, &attrs);
                }
            }
            "dp_style" => self.pending_dps = true,
            "para" => {
                if self.pending_dps {
                    self.pending_dps = false;
                    let dps = para_from_attrs(&attrs);
                    if let Some(shape) = &mut self.shape
                        && let Some(tf) = shape.tf.as_mut()
                    {
                        tf.dps = Some(dps);
                    }
                } else {
                    self.para_attrs = Some(attrs);
                }
            }
            "table" => {
                self.table_mode = Some(TableMode::ShapeTable);
                if let Some(shape) = &mut self.shape {
                    shape.table_grid = attr_str(&attrs, "grid");
                }
            }
            _ => {}
        }
    }

    // -- paragraphs ---------------------------------------------------------

    fn ensure_para(&mut self) {
        if self.para.is_some() {
            return;
        }
        let attrs = self.para_attrs.take().unwrap_or_default();
        let level = if self.list_depth > 0 {
            self.list_depth as i32
        } else {
            attr_i64(&attrs, "level").unwrap_or(0) as i32
        };
        self.para = Some(ParaBuf {
            level,
            attrs,
            runs: Runs::new(),
        });
        if self.pending_dps {
            self.pending_dps = false;
            if let Some(shape) = &mut self.shape
                && let Some(tf) = shape.tf.as_mut()
            {
                tf.dps = Some(para_from_attrs(&HashMap::new()));
            }
        }
    }

    fn begin_paragraph(&mut self) {
        self.ensure_para();
    }

    fn end_paragraph(&mut self) {
        self.finalize_paragraph();
    }

    fn finalize_paragraph(&mut self) {
        let Some(para) = self.para.take() else {
            return;
        };
        let mut para = para;
        let value = paragraph_value(&mut para);
        if let Some(shape) = &mut self.shape {
            let tf = shape.tf.get_or_insert_with(TfBuf::default);
            tf.paragraphs.push(value);
        }
    }

    fn handle_inline(&mut self, inl: Inl) {
        if self.in_table {
            self.cell_tokens.push(inl);
            return;
        }
        self.ensure_para();
        let para = self.para.as_mut().expect("paragraph exists");
        match inl {
            Inl::Text(t) => para.runs.text(&t),
            Inl::Bold(on) => para.runs.bold(on),
            Inl::Italic(on) => para.runs.italic(on),
            Inl::SpanOpen(attrs) => para.runs.span_open(attrs),
            Inl::SpanClose => para.runs.span_close(),
            Inl::Br => para.runs.text("\n"),
        }
    }

    // -- tables -------------------------------------------------------------

    fn begin_table(&mut self) {
        self.end_paragraph();
        self.in_table = true;
        self.table_rows.clear();
        self.table_row.clear();
    }

    fn end_table(&mut self) {
        self.in_table = false;
        let rows = std::mem::take(&mut self.table_rows);
        match self.table_mode.take() {
            Some(TableMode::CoreProperties) => self.core_properties = table_pairs(rows),
            Some(TableMode::ThemeColors) => self.theme_colors = table_pairs(rows),
            Some(TableMode::ThemeFonts) => self.theme_fonts = table_pairs(rows),
            Some(TableMode::ShapeTable) => {
                if let Some(shape) = &mut self.shape {
                    shape.table = Some(build_shape_table(&rows, shape.table_grid.as_deref()));
                }
            }
            None => {}
        }
    }

    // -- finalization -------------------------------------------------------

    fn finalize_shape(&mut self) {
        let Some(shape) = self.shape.take() else {
            return;
        };
        if self.pending_dps {
            self.pending_dps = false;
        }
        let value = shape_value(&shape);
        let target: Option<&mut Vec<Value>> = if self.in_notes {
            self.slide.as_mut().and_then(|s| s.notes_shapes.as_mut())
        } else if self.slide.is_some() {
            self.slide.as_mut().map(|s| &mut s.shapes)
        } else {
            self.master.as_mut().map(|m| &mut m.shapes)
        };
        if let Some(shapes) = target {
            shapes.push(value);
        }
    }

    fn finalize_slide(&mut self) {
        self.finalize_shape();
        let Some(slide) = self.slide.take() else {
            return;
        };
        let mut obj = Map::new();
        obj.insert("shapes".into(), json!(slide.shapes));
        let bg = match slide.background.as_deref() {
            Some(bg) => {
                let (ty, color) = bg.split_once(':').unwrap_or((bg, ""));
                let upper = ty.to_ascii_uppercase();
                if upper == "NONE" || ty.is_empty() {
                    json!({ "fill": { "type": Value::Null } })
                } else if upper == "SOLID" {
                    json!({ "fill": { "type": "SOLID", "color": color } })
                } else {
                    json!({ "fill": { "type": upper } })
                }
            }
            None => json!({ "fill": { "type": Value::Null } }),
        };
        obj.insert("background".into(), bg);
        let notes = match slide.notes_shapes {
            Some(shapes) => json!({ "shapes": shapes }),
            None => Value::Null,
        };
        obj.insert("notes".into(), notes);
        self.slides.push(Value::Object(obj));
    }

    fn finalize_master(&mut self) {
        let Some(master) = self.master.take() else {
            return;
        };
        self.masters.push(json!({ "shapes": master.shapes }));
    }
}

// -- value builders ----------------------------------------------------------

fn shape_value(shape: &ShapeBuf) -> Value {
    let mut obj = Map::new();
    obj.insert(
        "shape_type".into(),
        json!(
            shape
                .shape_type
                .as_deref()
                .unwrap_or("")
                .to_ascii_uppercase()
        ),
    );
    if let Some(name) = &shape.name {
        obj.insert("name".into(), json!(name));
    }
    if let Some(v) = shape.x {
        obj.insert("left".into(), json!(v));
    }
    if let Some(v) = shape.y {
        obj.insert("top".into(), json!(v));
    }
    if let Some(v) = shape.w {
        obj.insert("width".into(), json!(v));
    }
    if let Some(v) = shape.h {
        obj.insert("height".into(), json!(v));
    }
    if let Some(v) = shape.rotation {
        obj.insert("rotation".into(), json!(v));
    }
    if let Some(v) = &shape.autoshape {
        obj.insert("auto_shape_type".into(), json!(v));
    }
    for (key, val) in [
        ("fill", &shape.fill),
        ("outline", &shape.outline),
        ("crop", &shape.crop),
    ] {
        if let Some(v) = val {
            obj.insert(key.into(), v.clone());
        }
    }
    if let Some(tf) = &shape.tf {
        let mut tf_obj = Map::new();
        tf_obj.insert("paragraphs".into(), json!(tf.paragraphs));
        for (k, v) in &tf.props {
            tf_obj.insert(k.clone(), v.clone());
        }
        if let Some(dps) = &tf.dps {
            tf_obj.insert("default_paragraph_style".into(), dps.clone());
        }
        obj.insert("text_frame".into(), Value::Object(tf_obj));
    }
    if let Some(table) = &shape.table {
        obj.insert("table".into(), table.clone());
    }
    Value::Object(obj)
}

fn paragraph_value(para: &mut ParaBuf) -> Value {
    let runs = para.runs.finish();
    let mut obj = Map::new();
    if !runs.is_empty() {
        obj.insert("runs".into(), json!(runs));
    }
    obj.insert("level".into(), json!(para.level));
    for (k, v) in &para.attrs {
        insert_para_attr(&mut obj, k, v);
    }
    Value::Object(obj)
}

fn insert_para_attr(obj: &mut Map<String, Value>, key: &str, value: &str) {
    match key {
        "alignment" => {
            obj.insert("alignment".into(), json!(value.to_ascii_uppercase()));
        }
        "level" => {
            if let Ok(n) = value.parse::<i64>() {
                obj.insert("level".into(), json!(n));
            }
        }
        "line_spacing" => {
            if let Ok(n) = value.parse::<f64>() {
                obj.insert("line_spacing".into(), json!(n));
            }
        }
        "space_before" | "space_after" => {
            if let Ok(n) = value.parse::<i64>() {
                obj.insert(key.into(), json!(n));
            }
        }
        "font_name" | "font_size" | "font_bold" | "font_italic" | "font_underline"
        | "font_color" => {
            let font = obj
                .entry("font")
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .expect("font is an object");
            match key {
                "font_name" => {
                    font.insert("name".into(), json!(value));
                }
                "font_size" => {
                    if let Ok(n) = value.parse::<i32>() {
                        font.insert("size".into(), json!(n));
                    }
                }
                "font_bold" => {
                    font.insert("bold".into(), json!(value == "true" || value == "1"));
                }
                "font_italic" => {
                    font.insert("italic".into(), json!(value == "true" || value == "1"));
                }
                "font_underline" => {
                    font.insert("underline".into(), json!(value == "true" || value == "1"));
                }
                "font_color" => {
                    font.insert("color".into(), color_from_attr(value));
                }
                _ => {}
            }
        }
        _ => {}
    }
}

/// Build a `default_paragraph_style` value from a `<!-- para: -->` comment.
fn para_from_attrs(attrs: &HashMap<String, String>) -> Value {
    let mut obj = Map::new();
    if let Some(a) = attrs.get("alignment") {
        obj.insert("alignment".into(), json!(a.to_ascii_uppercase()));
    }
    obj.insert("level".into(), json!(attr_i64(attrs, "level").unwrap_or(0)));
    if let Some(v) = attrs.get("line_spacing")
        && let Ok(n) = v.parse::<f64>()
    {
        obj.insert("line_spacing".into(), json!(n));
    }
    for key in ["space_before", "space_after"] {
        if let Some(v) = attrs.get(key)
            && let Ok(n) = v.parse::<i64>()
        {
            obj.insert(key.into(), json!(n));
        }
    }
    let mut font = Map::new();
    if let Some(v) = attrs.get("font_name") {
        font.insert("name".into(), json!(v));
    }
    if let Some(v) = attrs.get("font_size")
        && let Ok(n) = v.parse::<i32>()
    {
        font.insert("size".into(), json!(n));
    }
    for (key, field) in [
        ("font_bold", "bold"),
        ("font_italic", "italic"),
        ("font_underline", "underline"),
    ] {
        if let Some(v) = attrs.get(key) {
            font.insert(field.into(), json!(v == "true" || v == "1"));
        }
    }
    if let Some(v) = attrs.get("font_color") {
        font.insert("color".into(), color_from_attr(v));
    }
    if !font.is_empty() {
        obj.insert("font".into(), Value::Object(font));
    }
    Value::Object(obj)
}

fn apply_tf_props(props: &mut Map<String, Value>, attrs: &HashMap<String, String>) {
    for (k, v) in attrs {
        let val = match k.as_str() {
            "auto_size" | "vertical_anchor" => json!(v.to_ascii_uppercase()),
            "word_wrap" => json!(v == "1" || v == "true"),
            "margin_left" | "margin_right" | "margin_top" | "margin_bottom" => {
                json!(v.parse::<i64>().unwrap_or(0))
            }
            _ => continue,
        };
        props.insert(k.clone(), val);
    }
}

fn build_shape_table(rows: &[Vec<Value>], grid: Option<&str>) -> Value {
    let cols = rows.first().map(Vec::len).unwrap_or(0);
    let widths: Vec<i64> = grid
        .map(|g| g.split(',').filter_map(|w| w.trim().parse().ok()).collect())
        .unwrap_or_default();
    let grid_vals: Vec<Value> = (0..cols)
        .map(|i| {
            let width = widths
                .get(i)
                .copied()
                .or_else(|| widths.last().copied())
                .unwrap_or(914400);
            json!({ "width": width })
        })
        .collect();
    let row_vals: Vec<Value> = rows.iter().map(|cells| json!({ "cells": cells })).collect();
    json!({ "grid": grid_vals, "rows": row_vals })
}

fn cell_value(tokens: &[Inl]) -> Value {
    let mut paragraphs: Vec<Value> = Vec::new();
    let mut runs = Runs::new();
    for inl in tokens {
        match inl {
            Inl::Br => {
                paragraphs.push(json!({ "runs": runs.finish() }));
            }
            Inl::Text(t) => runs.text(t),
            Inl::Bold(on) => runs.bold(*on),
            Inl::Italic(on) => runs.italic(*on),
            Inl::SpanOpen(attrs) => runs.span_open(attrs.clone()),
            Inl::SpanClose => runs.span_close(),
        }
    }
    paragraphs.push(json!({ "runs": runs.finish() }));
    let has_text = paragraphs.iter().any(|p| {
        p.get("runs")
            .and_then(Value::as_array)
            .is_some_and(|r| !r.is_empty())
    });
    if !has_text {
        return json!({});
    }
    json!({ "text_frame": { "paragraphs": paragraphs } })
}

fn table_pairs(rows: Vec<Vec<Value>>) -> Vec<(String, String)> {
    rows.into_iter()
        .skip(1) // the markdown header row
        .filter_map(|cells| {
            let key = cell_text(cells.first());
            if key.is_empty() {
                return None;
            }
            let value = cells.get(1).map(|c| cell_text(Some(c))).unwrap_or_default();
            Some((key, value))
        })
        .collect()
}

fn cell_text(cell: Option<&Value>) -> String {
    let Some(tf) = cell
        .and_then(Value::as_object)
        .and_then(|c| c.get("text_frame"))
        .and_then(Value::as_object)
    else {
        return String::new();
    };
    let Some(paragraphs) = tf.get("paragraphs").and_then(Value::as_array) else {
        return String::new();
    };
    paragraphs
        .iter()
        .map(|p| {
            p.get("runs")
                .and_then(Value::as_array)
                .map(|runs| {
                    runs.iter()
                        .map(|r| r.get("text").and_then(Value::as_str).unwrap_or(""))
                        .collect::<String>()
                })
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join("")
}

fn pairs_to_map(pairs: Vec<(String, String)>) -> Map<String, Value> {
    let mut map = Map::new();
    for (k, v) in pairs {
        map.insert(k, json!(v));
    }
    map
}

// -- comment parsing ---------------------------------------------------------

fn parse_comment(text: &str) -> Option<(String, HashMap<String, String>)> {
    let t = text.trim();
    let t = t.strip_prefix("<!--")?.trim_start();
    let t = t.strip_suffix("-->")?.trim_end();
    let (key, rest) = match t.find(':') {
        Some(i) => (t[..i].trim(), t[i + 1..].trim()),
        None => (t, ""),
    };
    Some((key.to_string(), parse_attrs(rest)))
}

fn parse_attrs(s: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        let mut tok = String::new();
        let mut in_quote = false;
        while i < chars.len() {
            let c = chars[i];
            if c == '"' {
                in_quote = !in_quote;
                tok.push(c);
            } else if c.is_whitespace() && !in_quote {
                break;
            } else {
                tok.push(c);
            }
            i += 1;
        }
        if let Some(eq) = tok.find('=') {
            map.insert(tok[..eq].to_string(), unquote(&tok[eq + 1..]));
        }
    }
    map
}

fn unquote(v: &str) -> String {
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        let inner = &v[1..v.len() - 1];
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('\\') => out.push('\\'),
                    Some('"') => out.push('"'),
                    Some('-') => out.push('-'),
                    Some(other) => {
                        out.push('\\');
                        out.push(other);
                    }
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

fn attr_i64(attrs: &HashMap<String, String>, key: &str) -> Option<i64> {
    attrs.get(key).and_then(|v| v.parse().ok())
}

fn attr_f64(attrs: &HashMap<String, String>, key: &str) -> Option<f64> {
    attrs.get(key).and_then(|v| v.parse().ok())
}

fn attr_str(attrs: &HashMap<String, String>, key: &str) -> Option<String> {
    attrs.get(key).cloned().filter(|v| !v.is_empty())
}

fn attr_json(attrs: &HashMap<String, String>, key: &str) -> Option<Value> {
    let v = attrs.get(key)?;
    if !(v.starts_with('{') || v.starts_with('[')) {
        return None;
    }
    serde_json::from_str(v).ok()
}

// -- entry point -------------------------------------------------------------

pub fn parse(md: &str) -> Value {
    let mut parser = Parser::new();
    let mut events = MdParser::new_ext(md, Options::ENABLE_TABLES);
    for event in events.by_ref() {
        parser.handle(event);
    }
    parser.finish()
}
