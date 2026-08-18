use pulldown_cmark::{Event, Options, Parser as MdParser, Tag, TagEnd};
use serde_json::{Map, Value, json};

use super::error::{MdError, MdResult, MdSpan};
use super::markers::{
    ATTR_AUTO_SHAPE, ATTR_CLASS, ATTR_FILL, ATTR_GRID, ATTR_NAME, ATTR_TYPE, MARKER_BACKGROUND,
    MARKER_PARAGRAPH, MARKER_SHAPE, shape_type_from_token,
};
use super::style::{
    Styles, font_from_decls, has_para_decls, para_from_decls, parse_style_block, shape_attrs,
    shape_from_decls, tf_from_decls, unquote,
};

/// The parsed document plus the source span of every editable element, keyed by
/// its JSON path (e.g. `slides[0].shapes[1].text_frame.paragraphs[2]`). The
/// build command uses these to attach rustc-style diagnostics to failed edits.
#[derive(Debug)]
pub struct ParsedDoc {
    pub doc: Value,
    pub spans: Vec<(String, MdSpan)>,
}

/// An inline-level token extracted from the event stream.
#[derive(Debug, Clone)]
enum Inl {
    Text(String),
    Bold(bool),
    Italic(bool),
    /// An opening `<span>` carrying its resolved font map; an empty map is a
    /// bare run-boundary separator.
    SpanOpen(Map<String, Value>),
    SpanClose,
    /// A `<br>` inside a table cell: separates paragraphs.
    Br,
}

/// Assembles inline events into a sequence of run objects.
struct Runs {
    runs: Vec<Value>,
    cur: Option<Map<String, Value>>,
    bold: u32,
    italic: u32,
    span: Option<Map<String, Value>>,
    span_active: bool,
    text_buf: String,
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
            text_buf: String::new(),
        }
    }

    /// The concatenated plain text, for heading detection (`### Notes`).
    fn text(&self) -> &str {
        &self.text_buf
    }

    fn add_text(&mut self, t: &str) {
        if !t.is_empty() {
            self.text_buf.push_str(t);
        }
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
            self.flush();
            self.bold = self.bold.saturating_sub(1);
        }
    }

    fn italic(&mut self, on: bool) {
        if on {
            self.flush();
            self.italic += 1;
        } else {
            self.flush();
            self.italic = self.italic.saturating_sub(1);
        }
    }

    fn span_open(&mut self, attrs: Map<String, Value>) {
        self.flush();
        self.span = if attrs.is_empty() { None } else { Some(attrs) };
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

fn apply_inl(runs: &mut Runs, inl: Inl) {
    match inl {
        Inl::Text(t) => runs.add_text(&t),
        Inl::Bold(on) => runs.bold(on),
        Inl::Italic(on) => runs.italic(on),
        Inl::SpanOpen(attrs) => runs.span_open(attrs),
        Inl::SpanClose => runs.span_close(),
        Inl::Br => runs.add_text("\n"),
    }
}

/// A text frame under construction.
#[derive(Default)]
struct TfBuf {
    props: Map<String, Value>,
    dps: Option<Value>,
    paragraphs: Vec<Value>,
}

/// A shape under construction.
struct ShapeBuf {
    map: Map<String, Value>,
    grid: Option<String>,
    /// Frame body properties folded into the shape class, merged into the
    /// frame when its first paragraph appears.
    tf_props: Map<String, Value>,
    /// The default paragraph style folded into the shape class.
    tf_dps: Option<Value>,
    tf: Option<TfBuf>,
    table: Option<Value>,
    span: Option<MdSpan>,
}

/// A slide under construction.
struct SlideBuf {
    background: Value,
    shapes: Vec<Value>,
    notes_shapes: Option<Vec<Value>>,
}

/// A paragraph under construction.
struct ParaBuf {
    style: Map<String, Value>,
    runs: Runs,
    span: Option<MdSpan>,
}

pub struct Parser<'a> {
    styles: Styles,
    source: &'a str,
    slide_width: i64,
    slide_height: i64,
    core_properties: Option<Value>,
    theme: Value,
    masters: Vec<Value>,
    slides: Vec<Value>,

    master: Option<Vec<Value>>,
    slide: Option<SlideBuf>,
    in_notes: bool,

    heading_span: Option<MdSpan>,
    heading_level: Option<u32>,

    shape: Option<ShapeBuf>,
    para_attrs: Option<Map<String, Value>>,
    para: Option<ParaBuf>,

    in_table: bool,
    table_rows: Vec<Vec<Value>>,
    table_row: Vec<Value>,
    cell_tokens: Vec<Inl>,

    spans: Vec<(String, MdSpan)>,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str, styles: Styles) -> Self {
        Parser {
            styles,
            source,
            slide_width: 0,
            slide_height: 0,
            core_properties: None,
            theme: json!({ "colors": {}, "fonts": {} }),
            masters: Vec::new(),
            slides: Vec::new(),
            master: None,
            slide: None,
            in_notes: false,
            heading_span: None,
            heading_level: None,
            shape: None,
            para_attrs: None,
            para: None,
            in_table: false,
            table_rows: Vec::new(),
            table_row: Vec::new(),
            cell_tokens: Vec::new(),
            spans: Vec::new(),
        }
    }

    // -- events ------------------------------------------------------------

    fn handle(&mut self, event: Event, off: usize) -> MdResult<()> {
        match event {
            Event::Html(t) => return self.handle_comment(&t, off),
            Event::InlineHtml(t) => {
                let inl = self.inl_from_html(&t, off)?;
                self.handle_inline(inl);
                return Ok(());
            }
            Event::Text(t) => self.handle_inline(Inl::Text(t.to_string())),
            Event::Code(t) => self.handle_inline(Inl::Text(t.to_string())),
            Event::SoftBreak | Event::HardBreak => self.handle_inline(Inl::Text("\n".into())),
            Event::Start(Tag::Strong) => self.handle_inline(Inl::Bold(true)),
            Event::End(TagEnd::Strong) => self.handle_inline(Inl::Bold(false)),
            Event::Start(Tag::Emphasis) => self.handle_inline(Inl::Italic(true)),
            Event::End(TagEnd::Emphasis) => self.handle_inline(Inl::Italic(false)),
            Event::Start(Tag::Heading { level, .. }) => self.begin_heading(level as u32, off),
            Event::End(TagEnd::Heading(_)) => self.end_heading(),
            Event::Start(Tag::Paragraph) => self.begin_paragraph(off),
            Event::End(TagEnd::Paragraph) => self.finalize_paragraph(),
            Event::Start(Tag::Item) => self.begin_paragraph(off),
            Event::End(TagEnd::Item) => self.finalize_paragraph(),
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
        Ok(())
    }

    // -- headings ----------------------------------------------------------

    fn begin_heading(&mut self, level: u32, off: usize) {
        match level {
            1 => {
                self.finalize_shape();
                self.finalize_slide();
                self.finalize_master();
                self.master = Some(Vec::new());
                self.in_notes = false;
                self.heading_level = Some(1);
            }
            2 => {
                self.finalize_shape();
                self.finalize_slide();
                self.slide = Some(SlideBuf {
                    background: json!({ "fill": { "type": Value::Null } }),
                    shapes: Vec::new(),
                    notes_shapes: None,
                });
                self.in_notes = false;
                let span = self.span_at(off);
                self.heading_span = Some(span.clone());
                self.spans
                    .push((format!("slides[{}]", self.slides.len()), span));
                self.heading_level = Some(2);
            }
            _ => {
                self.begin_paragraph(off);
                self.heading_span = Some(self.span_at(off));
                self.heading_level = Some(level);
            }
        }
    }

    fn end_heading(&mut self) {
        match self.heading_level.take() {
            Some(1) => {}
            Some(2) => {}
            Some(_) => {
                let is_notes = self
                    .para
                    .as_ref()
                    .map(|p| p.runs.text().trim() == "Notes")
                    .unwrap_or(false);
                if is_notes {
                    self.para = None;
                    self.finalize_shape();
                    self.in_notes = true;
                    if let Some(slide) = &mut self.slide {
                        slide.notes_shapes.get_or_insert_with(Vec::new);
                    }
                    if let Some(span) = &self.heading_span {
                        self.spans
                            .push((format!("slides[{}].notes", self.slides.len()), span.clone()));
                    }
                } else {
                    self.finalize_paragraph();
                }
            }
            None => {}
        }
    }

    // -- comments ----------------------------------------------------------

    fn handle_comment(&mut self, text: &str, off: usize) -> MdResult<()> {
        let Some((key, attrs)) = parse_comment(text) else {
            return Ok(());
        };
        match key.as_str() {
            MARKER_SHAPE => self.begin_shape(&attrs, off)?,
            MARKER_PARAGRAPH => {
                if let Some(class) = attr_value(&attrs, ATTR_CLASS) {
                    let decls = self.resolve_class(class, off)?;
                    self.para_attrs = Some(para_from_decls(&decls));
                }
            }
            MARKER_BACKGROUND => {
                if let Some(fill) = attr_value(&attrs, ATTR_FILL)
                    && let Some(slide) = &mut self.slide
                {
                    slide.background = background_value(fill);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn begin_shape(&mut self, attrs: &[(String, String)], off: usize) -> MdResult<()> {
        self.finalize_shape();
        let decls = match attr_value(attrs, ATTR_CLASS) {
            Some(class) => self.resolve_class(class, off)?,
            None => Vec::new(),
        };
        let mut map = shape_from_decls(&decls);
        if let Some(ty) = attr_value(attrs, ATTR_TYPE) {
            map.insert(
                "shape_type".into(),
                Value::String(shape_type_from_token(ty)),
            );
        } else {
            map.insert("shape_type".into(), Value::String(String::new()));
        }
        if let Some(v) = attr_value(attrs, ATTR_AUTO_SHAPE) {
            map.insert("auto_shape_type".into(), Value::String(v.to_string()));
        }
        shape_attrs(&mut map, attrs);
        let grid = attr_value(attrs, ATTR_GRID).map(str::to_string);

        // Frame body properties and the default paragraph style fold into the
        // shape class; they materialize on the frame when its first paragraph
        // appears (a text frame exists only with content, matching the query
        // projection).
        let tf_props = tf_from_decls(&decls);
        let tf_dps = if has_para_decls(&decls) {
            Some(Value::Object(para_from_decls(&decls)))
        } else {
            None
        };
        self.shape = Some(ShapeBuf {
            map,
            grid,
            tf_props,
            tf_dps,
            tf: None,
            table: None,
            span: Some(self.span_at(off)),
        });
        Ok(())
    }

    fn resolve_class(&self, class: &str, off: usize) -> MdResult<Vec<super::style::Decl>> {
        self.styles.get(class).cloned().ok_or_else(|| {
            let span = self.span_at(off);
            MdError::at(format!("unknown style class '{class}'"), span)
        })
    }

    // -- inline ------------------------------------------------------------

    fn inl_from_html(&self, tag: &str, off: usize) -> MdResult<Inl> {
        if matches!(tag, "<br>" | "<br/>" | "<br />") {
            return Ok(Inl::Br);
        }
        if tag.starts_with("</") {
            return Ok(Inl::SpanClose);
        }
        let (attrs, class) = span_tag_parts(tag);
        if let Some(class) = class {
            let decls = self.resolve_class(&class, off)?;
            return Ok(Inl::SpanOpen(font_from_decls(&decls).unwrap_or_default()));
        }
        if attrs.is_empty() {
            return Ok(Inl::SpanOpen(Map::new()));
        }
        Ok(Inl::SpanOpen(font_from_span(&attrs)))
    }

    fn handle_inline(&mut self, inl: Inl) {
        if self.in_table {
            self.cell_tokens.push(inl);
            return;
        }
        match self.heading_level {
            // Heading text is structural only: `## Slide N` text is ignored.
            Some(1) | Some(2) => return,
            _ => {}
        }
        self.ensure_para();
        let para = self.para.as_mut().expect("paragraph exists");
        apply_inl(&mut para.runs, inl);
    }

    // -- paragraphs --------------------------------------------------------

    fn begin_paragraph(&mut self, off: usize) {
        if self.para.is_some() {
            return;
        }
        self.para = Some(ParaBuf {
            style: self.para_attrs.take().unwrap_or_else(level_zero),
            runs: Runs::new(),
            span: Some(self.span_at(off)),
        });
    }

    fn ensure_para(&mut self) {
        if self.para.is_none() {
            self.para = Some(ParaBuf {
                style: self.para_attrs.take().unwrap_or_else(level_zero),
                runs: Runs::new(),
                span: None,
            });
        }
    }

    fn finalize_paragraph(&mut self) {
        let Some(mut para) = self.para.take() else {
            return;
        };
        let runs = para.runs.finish();
        let mut obj = para.style;
        if !runs.is_empty() {
            obj.insert("runs".into(), Value::Array(runs));
        }
        let value = Value::Object(obj);

        let had_tf = self.shape.as_ref().and_then(|s| s.tf.as_ref()).is_some();
        if !had_tf {
            self.ensure_tf();
        }
        if let Some(shape) = &mut self.shape {
            let tf = shape.tf.get_or_insert_with(TfBuf::default);
            let p = tf.paragraphs.len();
            tf.paragraphs.push(value);
            if let Some(span) = para.span
                && let Some(prefix) = self.container_prefix()
            {
                let m = self.target_len();
                self.spans.push((
                    format!("{prefix}.shapes[{m}].text_frame.paragraphs[{p}]"),
                    span,
                ));
            }
        }
    }

    // -- tables ------------------------------------------------------------

    fn begin_table(&mut self) {
        self.finalize_paragraph();
        self.in_table = true;
        self.table_rows.clear();
        self.table_row.clear();
    }

    fn end_table(&mut self) {
        self.in_table = false;
        let rows = std::mem::take(&mut self.table_rows);
        if let Some(shape) = &mut self.shape {
            let grid = shape.grid.clone();
            shape.table = Some(build_shape_table(&rows, grid.as_deref()));
        }
    }

    // -- finalization ------------------------------------------------------

    fn ensure_tf(&mut self) {
        if self.shape.as_ref().and_then(|s| s.tf.as_ref()).is_none()
            && let Some(shape) = &mut self.shape
        {
            let props = std::mem::take(&mut shape.tf_props);
            let dps = shape.tf_dps.take();
            shape.tf = Some(TfBuf {
                props,
                dps,
                paragraphs: Vec::new(),
            });
        }
    }

    fn finalize_shape(&mut self) {
        self.finalize_paragraph();
        let Some(shape) = self.shape.take() else {
            return;
        };
        let value = shape_value(&shape);
        let m = self.target_len();
        let target: Option<&mut Vec<Value>> = if self.in_notes {
            self.slide.as_mut().and_then(|s| s.notes_shapes.as_mut())
        } else if self.slide.is_some() {
            self.slide.as_mut().map(|s| &mut s.shapes)
        } else {
            self.master.as_mut()
        };
        if let Some(shapes) = target {
            shapes.push(value);
            if let Some(span) = &shape.span
                && let Some(prefix) = self.container_prefix()
            {
                self.spans
                    .push((format!("{prefix}.shapes[{m}]"), span.clone()));
            }
        }
    }

    fn finalize_slide(&mut self) {
        self.finalize_shape();
        let Some(slide) = self.slide.take() else {
            return;
        };
        let mut obj = Map::new();
        obj.insert("shapes".into(), json!(slide.shapes));
        obj.insert("background".into(), slide.background);
        let notes = match slide.notes_shapes {
            Some(shapes) => json!({ "shapes": shapes }),
            None => Value::Null,
        };
        obj.insert("notes".into(), notes);
        self.slides.push(Value::Object(obj));
        self.in_notes = false;
    }

    fn finalize_master(&mut self) {
        let Some(master) = self.master.take() else {
            return;
        };
        self.masters.push(json!({ "shapes": master }));
    }

    fn finalize_all(&mut self) {
        self.finalize_shape();
        self.finalize_slide();
        self.finalize_master();
    }

    fn finish(self) -> Value {
        let mut root = Map::new();
        root.insert("slide_width".into(), json!(self.slide_width));
        root.insert("slide_height".into(), json!(self.slide_height));
        root.insert("slides".into(), json!(self.slides));
        root.insert("slide_masters".into(), json!(self.masters));
        root.insert("theme".into(), self.theme.clone());
        if let Some(cp) = &self.core_properties
            && !cp.is_null()
        {
            root.insert("core_properties".into(), cp.clone());
        }
        Value::Object(root)
    }

    // -- spans -------------------------------------------------------------

    fn container_prefix(&self) -> Option<String> {
        if self.in_notes {
            Some(format!("slides[{}].notes", self.slides.len()))
        } else if self.slide.is_some() {
            Some(format!("slides[{}]", self.slides.len()))
        } else if self.master.is_some() {
            Some(format!("slide_masters[{}]", self.masters.len()))
        } else {
            None
        }
    }

    fn target_len(&self) -> usize {
        if self.in_notes {
            self.slide
                .as_ref()
                .and_then(|s| s.notes_shapes.as_ref())
                .map(Vec::len)
                .unwrap_or(0)
        } else if self.slide.is_some() {
            self.slide.as_ref().map(|s| s.shapes.len()).unwrap_or(0)
        } else {
            self.master.as_ref().map(Vec::len).unwrap_or(0)
        }
    }

    fn span_at(&self, offset: usize) -> MdSpan {
        let offset = offset.min(self.source.len());
        let prefix = &self.source[..offset];
        let line = prefix.bytes().filter(|&b| b == b'\n').count() + 1;
        let line_start = prefix.rfind('\n').map(|i| i + 1).unwrap_or(0);
        let col = prefix[line_start..].chars().count() + 1;
        let line_end = self.source[offset..]
            .find('\n')
            .map(|i| offset + i)
            .unwrap_or(self.source.len());
        MdSpan {
            line,
            col,
            offset,
            len: line_end.saturating_sub(offset),
        }
    }
}

// -- value builders ----------------------------------------------------------

fn shape_value(shape: &ShapeBuf) -> Value {
    let mut obj = shape.map.clone();
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
    if let Some(t) = &shape.table {
        obj.insert("table".into(), t.clone());
    }
    Value::Object(obj)
}

fn level_zero() -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("level".into(), json!(0));
    m
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
                paragraphs.push(paragraph_value(runs.finish()));
            }
            _ => apply_inl(&mut runs, inl.clone()),
        }
    }
    paragraphs.push(paragraph_value(runs.finish()));
    json!({ "text_frame": { "paragraphs": paragraphs } })
}

/// A table-cell paragraph: `{}` when empty, `{"runs": [...]}` otherwise. This
/// matches the projected snapshot, where cell paragraph style is stripped and
/// empty `runs` arrays are dropped by serde.
fn paragraph_value(runs: Vec<Value>) -> Value {
    if runs.is_empty() {
        json!({})
    } else {
        json!({ "runs": runs })
    }
}

fn attr_value<'a>(attrs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

fn background_value(value: &str) -> Value {
    let (ty, color) = value.split_once(':').unwrap_or((value, ""));
    let upper = ty.to_ascii_uppercase();
    if upper == "SOLID" {
        json!({ "fill": { "type": "SOLID", "color": color } })
    } else if upper.is_empty() || upper == "NONE" {
        json!({ "fill": { "type": Value::Null } })
    } else {
        json!({ "fill": { "type": upper } })
    }
}

// -- inline parsing ----------------------------------------------------------

/// Legacy `<span data-*>` support; the current format resolves `<span
/// class="...">` against the style block.
fn font_from_span(attrs: &Map<String, Value>) -> Map<String, Value> {
    let mut font = Map::new();
    for (key, field) in [
        ("size", "size"),
        (ATTR_NAME, "name"),
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
    let mut font = Map::new();
    for (key, field) in [
        ("size", "size"),
        (ATTR_NAME, "name"),
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

/// Split `<span class="run-1" data-x="1">` into its `data-*` attribute map and
/// its class name.
fn span_tag_parts(tag: &str) -> (Map<String, Value>, Option<String>) {
    let inner = tag
        .strip_prefix("<span")
        .and_then(|t| t.strip_suffix('>'))
        .unwrap_or("");
    let mut class = None;
    let mut attrs = Map::new();
    for (k, v) in tokenize_attrs(inner) {
        let Some(v) = v else { continue };
        if k == ATTR_CLASS {
            class = Some(v);
            continue;
        }
        let key = k.strip_prefix("data-").unwrap_or(&k).to_string();
        let val = match key.as_str() {
            "size" => json!(v.parse::<i64>().unwrap_or(0)),
            "bold" | "italic" | "underline" => json!(v == "true" || v == "1"),
            _ => json!(v),
        };
        attrs.insert(key, val);
    }
    (attrs, class)
}

// -- comment parsing ---------------------------------------------------------

/// Parse a marker comment like `<!-- shape type="textbox" class="textbox-1"
/// name="X" -->` into its key (`shape`) and attribute pairs. Bare words (no
/// `=`) are kept as the key position; a key-less comment yields `(key, [])`.
fn parse_comment(text: &str) -> Option<(String, Vec<(String, String)>)> {
    let t = text.trim();
    let t = t.strip_prefix("<!--")?.trim_start();
    let t = t.strip_suffix("-->")?.trim_end();
    let toks = tokenize_attrs(t);
    let mut it = toks.into_iter();
    let (key, _) = it.next()?;
    let attrs: Vec<(String, String)> = it.filter_map(|(k, v)| v.map(|v| (k, v))).collect();
    Some((key, attrs))
}

/// Split a tag/comment body into `key=value` pairs, keeping bare tokens (no
/// `=`) as `(token, None)`. Quoted values may contain spaces and backslash
/// escapes (`\"`).
fn tokenize_attrs(s: &str) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
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
            if c == '\\' && i + 1 < chars.len() && chars[i + 1] == '"' && in_quote {
                tok.push(c);
                tok.push(chars[i + 1]);
                i += 2;
                continue;
            }
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
            out.push((tok[..eq].to_string(), Some(unquote(&tok[eq + 1..]))));
        } else if !tok.is_empty() {
            out.push((tok, None));
        }
    }
    out
}

// -- entry point -------------------------------------------------------------

/// Split leading YAML front matter (`---\n...\n---`) from the body, returning
/// the parsed value, the body and the byte length of the front matter (BOM
/// included). Missing or unterminated front matter yields no front value and
/// treats the whole source as the body.
fn split_front_matter(source: &str) -> (Option<Value>, &str, usize) {
    if !source.starts_with("---\n") {
        return (None, source, 0);
    }
    let mut pos = 4;
    while pos < source.len() {
        let nl = source[pos..]
            .find('\n')
            .map(|i| pos + i)
            .unwrap_or(source.len());
        let line = source[pos..nl].trim_end_matches('\r');
        if line.trim() == "---" {
            let yaml_str = &source[4..pos];
            let front = yaml_serde::from_str::<Value>(yaml_str).ok();
            return (front, &source[nl + 1..], nl + 1);
        }
        pos = nl + 1;
    }
    (None, source, 0)
}

pub fn parse(md: &str) -> MdResult<ParsedDoc> {
    let source = md.strip_prefix('\u{feff}').unwrap_or(md);
    let (front, body, front_len) = split_front_matter(source);
    let styles = parse_style_block(body);
    let mut parser = Parser::new(source, styles);

    if let Some(front) = front {
        if let Some(v) = front.get("pptx") {
            if let Some(w) = v.get("slide_width").and_then(Value::as_i64) {
                parser.slide_width = w;
            }
            if let Some(h) = v.get("slide_height").and_then(Value::as_i64) {
                parser.slide_height = h;
            }
        }
        parser.core_properties = front.get("core_properties").cloned();
        if let Some(t) = front.get("theme")
            && let Some(obj) = t.as_object()
        {
            let mut m = obj.clone();
            m.entry("colors").or_insert_with(|| json!({}));
            m.entry("fonts").or_insert_with(|| json!({}));
            parser.theme = Value::Object(m);
        }
    }

    let events = MdParser::new_ext(body, Options::ENABLE_TABLES).into_offset_iter();
    for (event, range) in events {
        let off = front_len + range.start;
        parser.handle(event, off)?;
    }
    parser.finalize_all();
    let spans = std::mem::take(&mut parser.spans);
    let doc = parser.finish();
    Ok(ParsedDoc { doc, spans })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{normalize, serialize};

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

    fn assert_roundtrips(v: Value) -> String {
        let md = serialize::serialize(&v);
        let reparsed = parse(&md).expect("reparse serialized markdown").doc;
        assert_eq!(
            canonical(&normalize::normalize(&reparsed)),
            canonical(&normalize::normalize(&v)),
            "round-trip mismatch\n--- markdown ---\n{md}"
        );
        md
    }

    /// A projected snapshot: a styled body paragraph, a plain one, and a table
    /// whose cells mix text and empty paragraphs (projected cell paragraphs are
    /// `{}` — style stripped, empty runs dropped).
    fn snapshot() -> Value {
        json!({
            "slide_width": 9144000,
            "slide_height": 6858000,
            "theme": { "colors": {}, "fonts": {} },
            "slide_masters": [],
            "slides": [
                {
                    "background": { "fill": { "type": null } },
                    "notes": null,
                    "shapes": [
                        {
                            "shape_type": "TEXT_BOX",
                            "name": "Body 1",
                            "left": 914400,
                            "top": 914400,
                            "width": 3657600,
                            "height": 914400,
                            "text_frame": {
                                "paragraphs": [
                                    {
                                        "level": 0,
                                        "alignment": "CENTER",
                                        "line_spacing": 1.5,
                                        "space_before": 1000,
                                        "space_after": 2000,
                                        "runs": [ { "text": "Spaced" } ]
                                    },
                                    { "level": 0, "runs": [ { "text": "Plain" } ] }
                                ]
                            }
                        },
                        {
                            "shape_type": "TABLE",
                            "name": "Table 2",
                            "left": 914400,
                            "top": 1828800,
                            "width": 3657600,
                            "height": 1371600,
                            "table": {
                                "grid": [ { "width": 1828800 }, { "width": 1828800 } ],
                                "rows": [
                                    {
                                        "cells": [
                                            { "text_frame": { "paragraphs": [
                                                { "runs": [ { "text": "H1" } ] },
                                                {},
                                                {}
                                            ] } },
                                            { "text_frame": { "paragraphs": [ {} ] } }
                                        ]
                                    },
                                    {
                                        "cells": [
                                            { "text_frame": { "paragraphs": [
                                                { "runs": [ { "text": "A" } ] }
                                            ] } },
                                            { "text_frame": { "paragraphs": [ {} ] } }
                                        ]
                                    }
                                ]
                            }
                        }
                    ]
                }
            ]
        })
    }

    #[test]
    fn styled_paragraph_and_empty_cells_roundtrip() {
        let md = assert_roundtrips(snapshot());
        assert!(
            md.contains("--pptx-space-before: 1000;"),
            "space_before must use the `--pptx-space-before` declaration, got:\n{md}"
        );
        assert!(
            md.contains("--pptx-space-after: 2000;"),
            "space_after must use the `--pptx-space-after` declaration"
        );
        assert!(
            !md.contains("space_before"),
            "underscore field names must not leak into the mirror"
        );
        assert!(
            md.contains("| H1<br><br> |"),
            "cell paragraphs (text + two empties) serialize as runs joined by <br>"
        );
    }
}
