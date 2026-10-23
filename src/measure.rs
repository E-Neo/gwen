//! Layout measurements: spatial, content, structural, and palette facts about a
//! deck's slides.
//!
//! `measure` reports numbers, never judgments. Boxes are turned into layered ink
//! (fill, line, text, image), then rasterized into a grid so overlapping
//! shapes are measured as a union. Master chrome (footer, logo, decoration) is
//! excluded; master content (for example the closing "Thank you") is included.
//!
//! Lengths are kept in inches internally because the renderer's spec is in
//! inches. The Markdown and JSON outputs convert them to centimetres.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::Value;

use crate::error::Result;
use crate::model::Project;
use crate::pages::PageSelection;
use crate::render;

const CM_PER_IN: f64 = 2.54;
const CM2_PER_IN2: f64 = CM_PER_IN * CM_PER_IN;

/// Terminal characters are about twice as tall as wide.
const CELL_ASPECT: f64 = 2.0;
const BASE_COLS: usize = 48;
const MIN_COLS: usize = 32;
const MAX_COLS: usize = 96;
const MIN_ROWS: usize = 8;
const MAX_ROWS: usize = 48;

const CHAR_WIDTH_FACTOR: f64 = 0.52;
const BOLD_WIDTH_EXTRA: f64 = 0.03;
const LINE_HEIGHT_FACTOR: f64 = 1.2;
const DEFAULT_FONT_PT: f64 = 18.0;

/// A cell counts as covered above this ink fraction.
const INK_EPS: f64 = 0.01;
/// Salience below this is perceptually quiet.
const QUIET: f64 = 0.08;
/// Edges within this distance share a gridline.
const ALIGN_TOL: f64 = 0.05;
/// Parallel edges farther than this are not a near miss.
const NEAR_MISS_MAX: f64 = 0.30;
/// A child is "inside" a container within this slack.
const CONTAIN_TOL: f64 = 0.02;
/// A pair overlaps when their painted ink intersects this share of the smaller.
const OVERLAP_FRACTION: f64 = 0.10;
/// A full-bleed fill is paper, not content.
const BACKGROUND_FRACTION: f64 = 0.95;
/// Maximum entries listed per category in the report.
const MAX_LIST: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    fn area(&self) -> f64 {
        self.w.max(0.0) * self.h.max(0.0)
    }
    fn right(&self) -> f64 {
        self.x + self.w
    }
    fn bottom(&self) -> f64 {
        self.y + self.h
    }
    fn union(self, other: Rect) -> Rect {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Rect {
            x,
            y,
            w: self.right().max(other.right()) - x,
            h: self.bottom().max(other.bottom()) - y,
        }
    }
    fn inset(&self, m: Margins) -> Rect {
        Rect {
            x: self.x + m.l,
            y: self.y + m.t,
            w: (self.w - m.l - m.r).max(0.01),
            h: (self.h - m.t - m.b).max(0.01),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Margins {
    pub l: f64,
    pub r: f64,
    pub t: f64,
    pub b: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    const BLACK: Rgb = Rgb { r: 0, g: 0, b: 0 };
    const WHITE: Rgb = Rgb {
        r: 255,
        g: 255,
        b: 255,
    };

    fn parse(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches('#');
        let s = if s.len() == 8 { &s[..6] } else { s };
        let parse = |part: &str| u8::from_str_radix(part, 16).ok();
        let (r, g, b) = match s.len() {
            3 => {
                let r = parse(&s[0..1])?;
                let g = parse(&s[1..2])?;
                let b = parse(&s[2..3])?;
                (r * 17, g * 17, b * 17)
            }
            6 => (parse(&s[0..2])?, parse(&s[2..4])?, parse(&s[4..6])?),
            _ => return None,
        };
        Some(Rgb { r, g, b })
    }

    fn hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    fn blend(self, other: Rgb, alpha: f64) -> Rgb {
        let a = alpha.clamp(0.0, 1.0);
        let mix = |from: u8, to: u8| ((from as f64) * (1.0 - a) + (to as f64) * a).round() as u8;
        Rgb {
            r: mix(self.r, other.r),
            g: mix(self.g, other.g),
            b: mix(self.b, other.b),
        }
    }

    fn linear(self) -> (f64, f64, f64) {
        fn one(v: u8) -> f64 {
            let c = v as f64 / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        (one(self.r), one(self.g), one(self.b))
    }

    fn luminance(self) -> f64 {
        let (r, g, b) = self.linear();
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }
}

fn contrast(a: Rgb, b: Rgb) -> f64 {
    let (la, lb) = (a.luminance(), b.luminance());
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

fn visual_weight(contrast: f64) -> f64 {
    ((contrast - 1.0) / 6.5).clamp(0.0, 1.0)
}

fn rgb_to_lab(c: Rgb) -> (f64, f64, f64) {
    let (r, g, b) = c.linear();
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
    let y = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 1.0;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
    let f = |t: f64| {
        if t > 0.008856 {
            t.cbrt()
        } else {
            7.787 * t + 16.0 / 116.0
        }
    };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    (116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz))
}

fn delta_e(a: (f64, f64, f64), b: (f64, f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2) + (a.2 - b.2).powi(2)).sqrt()
}

fn value_color(v: &Value) -> Option<Rgb> {
    v.as_str()
        .and_then(Rgb::parse)
        .or_else(|| v.get("color").and_then(Value::as_str).and_then(Rgb::parse))
}

fn parse_fill(v: Option<&Value>) -> Option<(Rgb, f64)> {
    let v = v?;
    if v.is_null() {
        return None;
    }
    if let Some(s) = v.as_str() {
        return Rgb::parse(s).map(|c| (c, 1.0));
    }
    if v.get("type").and_then(Value::as_str) == Some("none") {
        return None;
    }
    let color = value_color(v)?;
    let alpha = 1.0 - v.get("transparency").and_then(Value::as_f64).unwrap_or(0.0) / 100.0;
    if alpha <= 0.0 {
        None
    } else {
        Some((color, alpha.clamp(0.0, 1.0)))
    }
}

fn parse_line(v: Option<&Value>) -> Option<(Rgb, f64)> {
    let v = v?;
    if v.is_null() || v.get("type").and_then(Value::as_str) == Some("none") {
        return None;
    }
    let color = value_color(v).unwrap_or(Rgb::BLACK);
    let width_pt = v.get("width").and_then(Value::as_f64).unwrap_or(1.0);
    Some((color, (width_pt.max(0.25)) / 72.0))
}

fn top_or_opts<'a>(obj: &'a Value, key: &str) -> Option<&'a Value> {
    obj.get(key)
        .or_else(|| obj.get("options").and_then(|o| o.get(key)))
}

fn top_or_opts_num(obj: &Value, key: &str) -> Option<f64> {
    top_or_opts(obj, key).and_then(Value::as_f64)
}

fn top_or_opts_str<'a>(obj: &'a Value, key: &str) -> Option<&'a str> {
    top_or_opts(obj, key).and_then(Value::as_str)
}

fn top_or_opts_bool(obj: &Value, key: &str) -> Option<bool> {
    top_or_opts(obj, key).and_then(Value::as_bool)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextAlign {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextVAlign {
    Top,
    Middle,
    Bottom,
}

fn align_of(v: Option<&Value>) -> TextAlign {
    match v.and_then(Value::as_str).unwrap_or("left") {
        "center" => TextAlign::Center,
        "right" => TextAlign::Right,
        _ => TextAlign::Left,
    }
}

fn valign_of(v: Option<&Value>) -> TextVAlign {
    match v.and_then(Value::as_str).unwrap_or("top") {
        "middle" => TextVAlign::Middle,
        "bottom" => TextVAlign::Bottom,
        _ => TextVAlign::Top,
    }
}

fn parse_margin(v: Option<&Value>, rect: Rect) -> Margins {
    let Some(v) = v else {
        return Margins::default();
    };
    let mut vals = [0.0f64; 4]; // top, right, bottom, left
    if let Some(list) = v.as_array() {
        for (slot, item) in vals.iter_mut().zip(list.iter()) {
            *slot = item.as_f64().unwrap_or(0.0);
        }
    } else if let Some(n) = v.as_f64() {
        vals = [n; 4];
    } else {
        return Margins::default();
    }
    // Numbers in the spec are points when they are whole numbers (the renderer
    // converts unit strings such as `0.1cm` to inches). This matches
    // pptxgenjs, where numeric margins are points.
    let all_int = vals.iter().all(|n| n.fract().abs() < 1e-9);
    if all_int && vals.iter().any(|n| *n > 0.0) {
        for n in &mut vals {
            *n /= 72.0;
        }
    }
    let [t, r, b, l] = vals;
    let _ = rect;
    Margins { l, r, t, b }
}

#[derive(Debug, Clone)]
struct TextRun {
    text: String,
    font_pt: f64,
    bold: bool,
    color: Rgb,
    break_line: bool,
    soft_break_before: bool,
    bullet: bool,
    indent_level: usize,
    line_spacing_pt: Option<f64>,
    line_spacing_multiple: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Slide,
    Master,
}

#[derive(Debug, Clone)]
struct Item {
    rect: Rect,
    source: Source,
    chrome: bool,
    is_background: bool,
    image: bool,
    label: String,
    shape: String,
    text: String,
    runs: Vec<TextRun>,
    font_pt: f64,
    bold: bool,
    color: Rgb,
    align: TextAlign,
    valign: TextVAlign,
    margin: Margins,
    line_spacing_pt: Option<f64>,
    para_space_before: f64,
    fill: Option<(Rgb, f64)>,
    line: Option<(Rgb, f64)>,
}

fn runs_text(v: &Value) -> String {
    v.as_array()
        .map(|list| {
            list.iter()
                .map(|r| r.get("text").and_then(Value::as_str).unwrap_or(""))
                .collect::<String>()
        })
        .unwrap_or_default()
}

fn first_run_font(v: &Value) -> Option<f64> {
    v.as_array().and_then(|list| {
        list.iter().find_map(|run| {
            run.get("fontSize").and_then(Value::as_f64).or_else(|| {
                run.get("options")
                    .and_then(|o| o.get("fontSize"))
                    .and_then(Value::as_f64)
            })
        })
    })
}

fn parse_runs(obj: &Value, base_font: f64, base_bold: bool, base_color: Rgb) -> Vec<TextRun> {
    let mut out = Vec::new();
    if let Some(list) = obj.get("runs").and_then(Value::as_array) {
        for run in list {
            let text = run.get("text").and_then(Value::as_str).unwrap_or("");
            let font_pt = run
                .get("fontSize")
                .and_then(Value::as_f64)
                .or_else(|| {
                    run.get("options")
                        .and_then(|o| o.get("fontSize"))
                        .and_then(Value::as_f64)
                })
                .unwrap_or(base_font);
            let bold = run
                .get("bold")
                .and_then(Value::as_bool)
                .or_else(|| {
                    run.get("options")
                        .and_then(|o| o.get("bold"))
                        .and_then(Value::as_bool)
                })
                .unwrap_or(base_bold);
            let color = run
                .get("color")
                .and_then(Value::as_str)
                .and_then(Rgb::parse)
                .or_else(|| {
                    run.get("options")
                        .and_then(|o| o.get("color"))
                        .and_then(Value::as_str)
                        .and_then(Rgb::parse)
                })
                .unwrap_or(base_color);
            let bullet = run
                .get("bullet")
                .is_some_and(|b| b.as_bool() == Some(true) || b.is_object());
            let indent_level = run.get("indentLevel").and_then(Value::as_u64).unwrap_or(0) as usize;
            let line_spacing_pt = run.get("lineSpacing").and_then(Value::as_f64);
            let line_spacing_multiple = run.get("lineSpacingMultiple").and_then(Value::as_f64);
            if text.is_empty()
                && !run
                    .get("breakLine")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                && !run
                    .get("softBreakBefore")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            {
                continue;
            }
            out.push(TextRun {
                text: text.to_string(),
                font_pt,
                bold,
                color,
                break_line: run
                    .get("breakLine")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                soft_break_before: run
                    .get("softBreakBefore")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                bullet,
                indent_level,
                line_spacing_pt,
                line_spacing_multiple,
            });
        }
    }
    if out.is_empty()
        && let Some(text) = obj.get("text").and_then(Value::as_str)
        && !text.is_empty()
    {
        out.push(TextRun {
            text: text.to_string(),
            font_pt: base_font,
            bold: base_bold,
            color: base_color,
            break_line: false,
            soft_break_before: false,
            bullet: false,
            indent_level: 0,
            line_spacing_pt: None,
            line_spacing_multiple: None,
        });
    }
    out
}

fn parse_item(obj: &Value, source: Source, index: usize) -> Option<Item> {
    let rect = Rect {
        x: obj.get("x").and_then(Value::as_f64).unwrap_or(0.0),
        y: obj.get("y").and_then(Value::as_f64).unwrap_or(0.0),
        w: obj.get("w").and_then(Value::as_f64).unwrap_or(0.0),
        h: obj.get("h").and_then(Value::as_f64).unwrap_or(0.0),
    };
    let kind = obj.get("kind").and_then(Value::as_str).unwrap_or("");
    let ty = obj.get("type").and_then(Value::as_str).unwrap_or("");
    let shape = obj
        .get("shape")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or(if ty.is_empty() { None } else { Some(ty) })
        .unwrap_or(kind)
        .to_string();
    let text = obj
        .get("runs")
        .map(runs_text)
        .or_else(|| obj.get("text").and_then(Value::as_str).map(String::from))
        .unwrap_or_default();
    let font_pt = top_or_opts_num(obj, "fontSize")
        .or_else(|| obj.get("runs").and_then(first_run_font))
        .unwrap_or(DEFAULT_FONT_PT);
    let color = top_or_opts_str(obj, "color")
        .and_then(Rgb::parse)
        .unwrap_or(Rgb::BLACK);
    let bold = top_or_opts_bool(obj, "bold").unwrap_or(false);
    let align = align_of(top_or_opts(obj, "align"));
    let valign = valign_of(top_or_opts(obj, "valign"));
    let margin = parse_margin(top_or_opts(obj, "margin"), rect);
    let line_spacing_pt = top_or_opts_num(obj, "lineSpacing");
    let para_space_before = top_or_opts_num(obj, "paraSpaceBefore").unwrap_or(0.0) / 72.0;
    let fill = parse_fill(top_or_opts(obj, "fill"));
    let line = parse_line(top_or_opts(obj, "line"));
    let image = kind == "image" || ty == "image";
    let runs = parse_runs(obj, font_pt, bold, color);
    Some(Item {
        rect,
        source,
        chrome: false,
        is_background: false,
        image,
        label: format!("{shape}#{}", index + 1),
        shape,
        text,
        runs,
        font_pt,
        bold,
        color,
        align,
        valign,
        margin,
        line_spacing_pt,
        para_space_before,
        fill,
        line,
    })
}

#[derive(Debug, Clone)]
struct Placeholder {
    rect: Rect,
    font_pt: f64,
    color: Rgb,
    bold: bool,
    align: TextAlign,
    valign: TextVAlign,
    margin: Margins,
    line_spacing_pt: Option<f64>,
    para_space_before: f64,
}

fn placeholder_info(spec: &Value, master: &str, name: &str) -> Option<Placeholder> {
    let list = spec.get("masters").and_then(Value::as_array)?;
    let m = list
        .iter()
        .find(|m| m.get("name").and_then(Value::as_str) == Some(master))?;
    let objects = m.get("objects").and_then(Value::as_array)?;
    for obj in objects {
        if obj.get("type").and_then(Value::as_str) != Some("placeholder") {
            continue;
        }
        let opts = obj.get("options");
        let matches = opts
            .and_then(|o| o.get("name"))
            .and_then(Value::as_str)
            .or_else(|| {
                opts.and_then(|o| o.get("placeholder"))
                    .and_then(Value::as_str)
            })
            == Some(name);
        if !matches {
            continue;
        }
        let rect = Rect {
            x: obj.get("x").and_then(Value::as_f64).unwrap_or(0.0),
            y: obj.get("y").and_then(Value::as_f64).unwrap_or(0.0),
            w: obj.get("w").and_then(Value::as_f64).unwrap_or(0.0),
            h: obj.get("h").and_then(Value::as_f64).unwrap_or(0.0),
        };
        let font_pt = top_or_opts_num(obj, "fontSize").unwrap_or(DEFAULT_FONT_PT);
        let color = top_or_opts_str(obj, "color")
            .and_then(Rgb::parse)
            .unwrap_or(Rgb::BLACK);
        let bold = top_or_opts_bool(obj, "bold").unwrap_or(false);
        let align = align_of(top_or_opts(obj, "align"));
        let valign = valign_of(top_or_opts(obj, "valign"));
        let margin = parse_margin(top_or_opts(obj, "margin"), rect);
        let line_spacing_pt = top_or_opts_num(obj, "lineSpacing");
        let para_space_before = top_or_opts_num(obj, "paraSpaceBefore").unwrap_or(0.0) / 72.0;
        return Some(Placeholder {
            rect,
            font_pt,
            color,
            bold,
            align,
            valign,
            margin,
            line_spacing_pt,
            para_space_before,
        });
    }
    None
}

fn slide_items(spec: &Value, slide: &Value, master: &str) -> Vec<Item> {
    let shapes = slide
        .get("shapes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for (i, sh) in shapes.iter().enumerate() {
        let Some(mut item) = parse_item(sh, Source::Slide, i) else {
            continue;
        };
        if let Some(name) = sh.get("placeholder").and_then(Value::as_str)
            && let Some(p) = placeholder_info(spec, master, name)
        {
            let has_geom = sh.get("w").and_then(Value::as_f64).unwrap_or(0.0) > 0.0
                && sh.get("h").and_then(Value::as_f64).unwrap_or(0.0) > 0.0;
            if !has_geom {
                item.rect = p.rect;
            }
            if sh.get("fontSize").is_none() && sh.get("runs").and_then(first_run_font).is_none() {
                item.font_pt = p.font_pt;
            }
            if sh.get("color").is_none() {
                item.color = p.color;
            }
            if sh.get("bold").is_none() {
                item.bold = p.bold;
            }
            if sh.get("align").is_none() {
                item.align = p.align;
            }
            if sh.get("valign").is_none() {
                item.valign = p.valign;
            }
            if sh.get("margin").is_none() {
                item.margin = p.margin;
            }
            if sh.get("lineSpacing").is_none() {
                item.line_spacing_pt = p.line_spacing_pt;
            }
            if sh.get("paraSpaceBefore").is_none() {
                item.para_space_before = p.para_space_before;
            }
            item.runs = parse_runs(sh, item.font_pt, item.bold, item.color);
        }
        if item.rect.w > 0.0 && item.rect.h > 0.0 {
            out.push(item);
        }
    }
    out
}

fn master_items(spec: &Value, width: f64, height: f64) -> BTreeMap<String, Vec<Item>> {
    let mut out = BTreeMap::new();
    let slide_area = width * height;
    if let Some(list) = spec.get("masters").and_then(Value::as_array) {
        for m in list {
            let name = m.get("name").and_then(Value::as_str).unwrap_or("");
            let mut items = Vec::new();
            if let Some(objects) = m.get("objects").and_then(Value::as_array) {
                for (i, obj) in objects.iter().enumerate() {
                    if obj.get("type").and_then(Value::as_str) == Some("placeholder") {
                        continue;
                    }
                    let Some(mut item) = parse_item(obj, Source::Master, i) else {
                        continue;
                    };
                    // Master chrome: footer/logo, tiny marks, and decorative
                    // shapes with no text. Master text that is large and not
                    // in the footer (the closing slide's "Thank you") is
                    // content, not chrome.
                    item.chrome = item.image
                        || item.rect.y >= height * 0.82
                        || item.font_pt < 24.0
                        || item.text.trim().is_empty()
                        || item.rect.area() < slide_area * 0.01;
                    if item.rect.w > 0.0 && item.rect.h > 0.0 {
                        items.push(item);
                    }
                }
            }
            out.insert(name.to_string(), items);
        }
    }
    out
}

fn background_color(v: Option<&Value>) -> Option<Rgb> {
    let v = v?;
    if v.is_null() {
        return None;
    }
    value_color(v)
}

fn master_background(spec: &Value, name: &str) -> Option<Rgb> {
    spec.get("masters")
        .and_then(Value::as_array)?
        .iter()
        .find(|m| m.get("name").and_then(Value::as_str) == Some(name))
        .and_then(|m| background_color(m.get("background")))
}

// ---------------------------------------------------------------------------
// Text layout

#[derive(Debug, Clone)]
pub struct TextLine {
    pub rect: Rect,
    pub color: Rgb,
    pub font_pt: f64,
    pub text: String,
    pub words: usize,
}

#[derive(Debug, Clone, Default)]
pub struct TextLayout {
    pub lines: Vec<TextLine>,
    pub words: usize,
    pub chars: usize,
    pub area: f64,
    pub overflow: bool,
}

fn is_cjk(c: char) -> bool {
    matches!(
        c as u32,
        0x2E80..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF
    )
}

fn count_words(text: &str) -> usize {
    let mut n = 0usize;
    let mut in_token = false;
    for c in text.chars() {
        if c.is_whitespace() {
            in_token = false;
        } else if is_cjk(c) {
            n += 1;
            in_token = false;
        } else if !in_token {
            n += 1;
            in_token = true;
        }
    }
    n
}

fn count_chars(text: &str) -> usize {
    text.chars()
        .filter(|c| !c.is_whitespace() && *c != '•')
        .count()
}

fn char_width(c: char, font_pt: f64, bold: bool) -> f64 {
    let em = font_pt / 72.0;
    if c == ' ' {
        0.28 * em
    } else if c == '\t' {
        0.5 * em
    } else if is_cjk(c) {
        em
    } else {
        (CHAR_WIDTH_FACTOR + if bold { BOLD_WIDTH_EXTRA } else { 0.0 }) * em
    }
}

fn layout_text(item: &Item) -> TextLayout {
    if item.text.trim().is_empty() || item.runs.is_empty() {
        return TextLayout::default();
    }
    let inner = item.rect.inset(item.margin);
    let mut paragraphs: Vec<Vec<TextRun>> = Vec::new();
    let mut current: Vec<TextRun> = Vec::new();
    for run in &item.runs {
        if run.soft_break_before && !current.is_empty() {
            current.push(TextRun {
                text: "\n".into(),
                font_pt: run.font_pt,
                bold: run.bold,
                color: run.color,
                break_line: false,
                soft_break_before: false,
                bullet: false,
                indent_level: 0,
                line_spacing_pt: None,
                line_spacing_multiple: None,
            });
        }
        current.push(run.clone());
        if run.break_line {
            paragraphs.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        paragraphs.push(current);
    }

    struct LineAcc {
        text: String,
        width: f64,
        font_pt: f64,
        color: Rgb,
        started: bool,
    }
    impl LineAcc {
        fn new(font_pt: f64, color: Rgb) -> Self {
            Self {
                text: String::new(),
                width: 0.0,
                font_pt,
                color,
                started: false,
            }
        }
        fn push(&mut self, text: &str, width: f64, font_pt: f64, color: Rgb) {
            self.text.push_str(text);
            self.width += width;
            if font_pt >= self.font_pt {
                self.font_pt = font_pt;
                self.color = color;
            }
            self.started = true;
        }
    }

    let mut raw_lines: Vec<(LineAcc, f64, f64)> = Vec::new(); // line, top, height
    let mut cursor_y = 0.0f64;
    let mut max_width = 0.0f64;

    for (pi, para) in paragraphs.iter().enumerate() {
        if para.iter().all(|r| r.text.trim().is_empty()) {
            continue;
        }
        let first = &para[0];
        let bullet = first.bullet;
        let indent = if bullet {
            0.22 * (first.indent_level as f64 + 1.0)
        } else {
            0.0
        };
        let usable = (inner.w - indent).max(0.05);
        let spacing_mult = first.line_spacing_multiple.unwrap_or(1.0).max(0.8);
        let spacing_pt = first
            .line_spacing_pt
            .or(item.line_spacing_pt)
            .unwrap_or(0.0)
            .max(0.0);

        if pi > 0 {
            cursor_y += item.para_space_before.max(0.0);
        }
        let mut line = LineAcc::new(item.font_pt, item.color);
        if bullet {
            line.push("• ", 0.22, item.font_pt, item.color);
        }

        let mut flush = |line: &mut LineAcc, y: &mut f64| {
            if !line.started || line.text.trim().is_empty() {
                *line = LineAcc::new(item.font_pt, item.color);
                return;
            }
            let h = (line.font_pt * LINE_HEIGHT_FACTOR * spacing_mult / 72.0)
                .max(spacing_pt / 72.0)
                .max(0.01);
            max_width = max_width.max(line.width);
            let acc = std::mem::replace(line, LineAcc::new(item.font_pt, item.color));
            raw_lines.push((acc, *y, h));
            *y += h;
        };

        for run in para {
            if run.soft_break_before {
                flush(&mut line, &mut cursor_y);
            }
            for chunk in run.text.split('\n') {
                if chunk.is_empty() {
                    flush(&mut line, &mut cursor_y);
                    continue;
                }
                for token in chunk.split_inclusive(char::is_whitespace) {
                    let token_width: f64 = token
                        .chars()
                        .map(|c| char_width(c, run.font_pt, run.bold))
                        .sum();
                    if line.started && line.width + token_width > usable {
                        flush(&mut line, &mut cursor_y);
                    }
                    if token_width > usable {
                        for c in token.chars() {
                            let w = char_width(c, run.font_pt, run.bold);
                            if line.started && line.width + w > usable && c != ' ' {
                                flush(&mut line, &mut cursor_y);
                            }
                            if !line.started && c == ' ' {
                                continue;
                            }
                            line.push(&c.to_string(), w, run.font_pt, run.color);
                        }
                    } else {
                        if !line.started && token.starts_with(' ') {
                            if let Some(trimmed) = token.trim_start().strip_suffix(' ') {
                                let w: f64 = trimmed
                                    .chars()
                                    .map(|c| char_width(c, run.font_pt, run.bold))
                                    .sum();
                                if !trimmed.is_empty() {
                                    line.push(trimmed, w, run.font_pt, run.color);
                                }
                            }
                        } else {
                            line.push(token, token_width, run.font_pt, run.color);
                        }
                    }
                }
            }
            if run.break_line {
                break;
            }
        }
        flush(&mut line, &mut cursor_y);
    }

    let block_h = cursor_y.max(0.0);
    let valign_offset = match item.valign {
        TextVAlign::Top => 0.0,
        TextVAlign::Middle => ((inner.h - block_h) / 2.0).max(0.0),
        TextVAlign::Bottom => (inner.h - block_h).max(0.0),
    };
    let mut lines = Vec::with_capacity(raw_lines.len());
    let mut area = 0.0;
    let mut words = 0usize;
    let mut chars = 0usize;
    for (mut acc, top, h) in raw_lines {
        acc.text = acc.text.trim_end().to_string();
        let width = acc.width.min(inner.w).max(0.01);
        let x = match item.align {
            TextAlign::Left => inner.x,
            TextAlign::Center => inner.x + (inner.w - width).max(0.0) / 2.0,
            TextAlign::Right => inner.right() - width,
        };
        let rect = Rect {
            x,
            y: inner.y + valign_offset + top,
            w: width,
            h,
        };
        let words_in_line = count_words(acc.text.trim_start_matches('•').trim());
        words += words_in_line;
        chars += count_chars(&acc.text);
        area += rect.w * rect.h;
        lines.push(TextLine {
            rect,
            color: acc.color,
            font_pt: acc.font_pt,
            text: acc.text,
            words: words_in_line,
        });
    }

    TextLayout {
        overflow: block_h > inner.h + 0.01 || max_width > inner.w + 0.01,
        lines,
        words,
        chars,
        area,
    }
}

// ---------------------------------------------------------------------------
// Ink grid

fn grid_dims(width: f64, height: f64) -> (usize, usize) {
    let aspect = (width / height).max(0.2);
    let cols = 96usize;
    let rows = ((cols as f64 / aspect).round() as usize).clamp(24, 160);
    (cols, rows)
}

fn heat_dims(width: f64, height: f64) -> (usize, usize) {
    let aspect = (width / height).max(0.2);
    let cols = BASE_COLS.clamp(MIN_COLS, MAX_COLS);
    let rows = ((cols as f64 / (CELL_ASPECT * aspect)).round() as usize).clamp(MIN_ROWS, MAX_ROWS);
    (cols, rows)
}

struct Grid {
    cols: usize,
    rows: usize,
    cells: Vec<f32>,
}

impl Grid {
    fn new(cols: usize, rows: usize) -> Self {
        Self {
            cols,
            rows,
            cells: vec![0.0; cols * rows],
        }
    }

    fn rasterize(&mut self, slide: Rect, r: Rect, weight: f64) {
        if r.w <= 0.0 || r.h <= 0.0 || weight <= 0.0 {
            return;
        }
        let cw = slide.w / self.cols as f64;
        let ch = slide.h / self.rows as f64;
        let x0 = ((r.x - slide.x) / cw).floor().max(0.0) as usize;
        let y0 = ((r.y - slide.y) / ch).floor().max(0.0) as usize;
        let x1 = (((r.right() - slide.x) / cw).ceil() as usize).min(self.cols);
        let y1 = (((r.bottom() - slide.y) / ch).ceil() as usize).min(self.rows);
        for row in y0..y1 {
            for col in x0..x1 {
                let cell = Rect {
                    x: slide.x + col as f64 * cw,
                    y: slide.y + row as f64 * ch,
                    w: cw,
                    h: ch,
                };
                let frac = overlap_area(cell, r) / (cw * ch);
                let value = (weight * frac) as f32;
                let idx = row * self.cols + col;
                if value > self.cells[idx] {
                    self.cells[idx] = value;
                }
            }
        }
    }

    fn value(&self, col: usize, row: usize) -> f64 {
        self.cells[row * self.cols + col] as f64
    }

    fn total(&self) -> f64 {
        self.cells.iter().map(|c| *c as f64).sum()
    }

    fn covered(&self) -> usize {
        self.cells.iter().filter(|c| **c as f64 >= INK_EPS).count()
    }

    fn bbox(&self, slide: Rect) -> Option<Rect> {
        let cw = slide.w / self.cols as f64;
        let ch = slide.h / self.rows as f64;
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for row in 0..self.rows {
            for col in 0..self.cols {
                if self.value(col, row) >= INK_EPS {
                    x0 = x0.min(col as f64 * cw);
                    y0 = y0.min(row as f64 * ch);
                    x1 = x1.max((col + 1) as f64 * cw);
                    y1 = y1.max((row + 1) as f64 * ch);
                }
            }
        }
        if x1 < x0 {
            None
        } else {
            Some(Rect {
                x: slide.x + x0,
                y: slide.y + y0,
                w: x1 - x0,
                h: y1 - y0,
            })
        }
    }

    fn downsampled(&self, cols: usize, rows: usize) -> Grid {
        let mut out = Grid::new(cols, rows);
        for row in 0..rows {
            for col in 0..cols {
                let c0 = col * self.cols / cols;
                let c1 = ((col + 1) * self.cols / cols).max(c0 + 1);
                let r0 = row * self.rows / rows;
                let r1 = ((row + 1) * self.rows / rows).max(r0 + 1);
                let mut sum = 0.0;
                let mut n = 0usize;
                for r in r0..r1.min(self.rows) {
                    for c in c0..c1.min(self.cols) {
                        sum += self.value(c, r);
                        n += 1;
                    }
                }
                out.cells[row * cols + col] = if n == 0 { 0.0 } else { (sum / n as f64) as f32 };
            }
        }
        out
    }
}

fn overlap_area(a: Rect, b: Rect) -> f64 {
    let w = (a.right().min(b.right()) - a.x.max(b.x)).max(0.0);
    let h = (a.bottom().min(b.bottom()) - a.y.max(b.y)).max(0.0);
    w * h
}

fn large_quiet_rect(grid: &Grid) -> Option<(Rect, usize, usize, usize, usize)> {
    let mut heights = vec![0usize; grid.cols];
    let mut best = (0usize, 0usize, 0usize, 0usize, 0usize);
    let mut best_area = 0usize;
    for row in 0..grid.rows {
        for (col, height) in heights.iter_mut().enumerate() {
            *height = if grid.value(col, row) < QUIET {
                *height + 1
            } else {
                0
            };
        }
        let mut stack: Vec<(usize, usize)> = Vec::new();
        for (col, h) in heights
            .iter()
            .copied()
            .chain(std::iter::once(0))
            .enumerate()
        {
            let mut start = col;
            while let Some(&(s, sh)) = stack.last() {
                if sh > h {
                    let area = sh * (col - s);
                    if area > best_area {
                        best_area = area;
                        best = (s, row + 1 - sh, col - s, sh, area);
                    }
                    start = s;
                    stack.pop();
                } else {
                    break;
                }
            }
            if h > 0 && stack.last().is_none_or(|&(_, sh)| sh != h) {
                stack.push((start, h));
            }
        }
    }
    if best_area == 0 {
        None
    } else {
        Some((
            Rect {
                x: 0.0,
                y: 0.0,
                w: best.2 as f64,
                h: best.3 as f64,
            },
            best.0,
            best.1,
            best.2,
            best.3,
        ))
    }
}

// ---------------------------------------------------------------------------
// Public measurements

#[derive(Debug, Clone, Copy, Default)]
pub struct Void {
    pub share: f64,
    pub rect: Rect,
    pub edge: bool,
}

#[derive(Debug, Clone)]
pub struct Composition {
    pub coverage: f64,
    pub whitespace: f64,
    pub largest_void: Option<Void>,
    pub salience: f64,
    pub balance_x: f64,
    pub balance_y: f64,
    pub margins: Option<Margins>,
    pub zones: [[f64; 3]; 3],
}

#[derive(Debug, Clone)]
pub struct Decoration {
    pub label: String,
    pub rect: Rect,
    pub color: Option<Rgb>,
}

#[derive(Debug, Clone)]
pub struct Content {
    pub words: usize,
    pub chars: usize,
    pub lines: usize,
    pub text_area: f64,
    pub content_area: f64,
    pub density: f64,
    pub content_density: f64,
    pub decoration_share: f64,
    pub decoration_count: usize,
    pub decorations: Vec<Decoration>,
    pub zones_words: [[usize; 3]; 3],
}

#[derive(Debug, Clone)]
pub struct Unaligned {
    pub item: String,
    pub side: String,
    pub at: f64,
    pub nearest: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct NearMiss {
    pub a: String,
    pub b: String,
    pub gap: f64,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct GapStats {
    pub median: Option<f64>,
    pub even: usize,
    pub total: usize,
    pub cv: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct Structure {
    pub alignment: f64,
    pub unaligned_count: usize,
    pub unaligned: Vec<Unaligned>,
    pub x_lines: Vec<(f64, usize)>,
    pub y_lines: Vec<(f64, usize)>,
    pub near_miss_count: usize,
    pub near_miss: Vec<NearMiss>,
    pub vertical: GapStats,
    pub horizontal: GapStats,
    pub collision_count: usize,
    pub collisions: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ColorShare {
    pub color: Rgb,
    pub share: f64,
}

#[derive(Debug, Clone)]
pub struct Palette {
    pub families: usize,
    pub raw_colors: usize,
    pub neutrals: usize,
    pub accents: usize,
    pub top: Vec<ColorShare>,
    pub contrast_min: Option<f64>,
    pub low_contrast_runs: usize,
    pub one_off: Vec<Rgb>,
}

#[derive(Debug, Clone)]
pub struct Heatmap {
    pub cols: usize,
    pub rows: usize,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SlideMeasure {
    pub slide: String,
    pub master: Option<String>,
    pub composition: Composition,
    pub content: Content,
    pub structure: Structure,
    pub palette: Palette,
    pub heatmap: Heatmap,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DeckSummary {
    pub whitespace: f64,
    pub salience: f64,
    pub words: f64,
    pub coverage: f64,
}

#[derive(Debug, Clone)]
pub struct Measurement {
    pub title: String,
    pub width: f64,
    pub height: f64,
    pub page: Option<Vec<usize>>,
    pub total_slides: usize,
    pub deck: DeckSummary,
    pub slides: Vec<SlideMeasure>,
}

/// Measure every slide of the project at `project_dir`.
pub fn measure(project_dir: &Path) -> Result<Measurement> {
    measure_with(project_dir, None)
}

/// Measure every slide, or only the selected `pages` (1-based) when given.
/// Deck averages always cover the whole deck.
pub fn measure_with(project_dir: &Path, pages: Option<&PageSelection>) -> Result<Measurement> {
    let project = Project::load(project_dir)?;
    let spec = render::spec_json(&project)?;
    let spec: Value = serde_json::from_str(&spec)
        .map_err(|e| miette::miette!("cannot parse the deck spec: {e}"))?;
    let title = project.main.presentation.title.clone();
    let mut measurement = measure_spec(&spec, title);
    if let Some(pages) = pages {
        let selected = pages
            .select(measurement.total_slides)
            .map_err(|e| miette::miette!(e))?;
        measurement.slides = selected
            .iter()
            .map(|page| measurement.slides[page - 1].clone())
            .collect();
        measurement.page = Some(selected);
    }
    Ok(measurement)
}

fn deck_summary(slides: &[SlideMeasure]) -> DeckSummary {
    if slides.is_empty() {
        return DeckSummary::default();
    }
    let n = slides.len() as f64;
    DeckSummary {
        whitespace: slides.iter().map(|s| s.composition.whitespace).sum::<f64>() / n,
        salience: slides.iter().map(|s| s.composition.salience).sum::<f64>() / n,
        words: slides.iter().map(|s| s.content.words as f64).sum::<f64>() / n,
        coverage: slides.iter().map(|s| s.composition.coverage).sum::<f64>() / n,
    }
}

fn measure_spec(spec: &Value, title: String) -> Measurement {
    let width = spec.get("width").and_then(Value::as_f64).unwrap_or(13.333);
    let height = spec.get("height").and_then(Value::as_f64).unwrap_or(7.5);
    let masters = master_items(spec, width, height);

    let mut slides = Vec::new();
    if let Some(sections) = spec.get("sections").and_then(Value::as_array) {
        for section in sections {
            let Some(list) = section.get("slides").and_then(Value::as_array) else {
                continue;
            };
            for slide in list {
                slides.push(measure_slide(spec, slide, &masters, width, height));
            }
        }
    }
    let total_slides = slides.len();
    let deck = deck_summary(&slides);
    Measurement {
        title,
        width,
        height,
        page: None,
        total_slides,
        deck,
        slides,
    }
}

fn measure_slide(
    spec: &Value,
    slide: &Value,
    masters: &BTreeMap<String, Vec<Item>>,
    width: f64,
    height: f64,
) -> SlideMeasure {
    let name = slide
        .get("file")
        .and_then(Value::as_str)
        .unwrap_or("slide")
        .to_string();
    let master = slide
        .get("master")
        .and_then(Value::as_str)
        .map(String::from);
    let slide_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: width,
        h: height,
    };
    let slide_area = slide_rect.area();

    let mut items = slide_items(spec, slide, master.as_deref().unwrap_or(""));
    if let Some(list) = master.as_ref().and_then(|m| masters.get(m)) {
        items.extend(list.iter().filter(|i| !i.chrome).cloned());
    }

    let mut paper = slide
        .get("background")
        .and_then(|b| background_color(Some(b)))
        .or_else(|| master.as_deref().and_then(|m| master_background(spec, m)))
        .unwrap_or(Rgb::WHITE);

    // A full-bleed fill is paper. If the slide does not declare a background,
    // that fill supplies the paper colour.
    let explicit_bg = slide
        .get("background")
        .and_then(|b| background_color(Some(b)))
        .is_some();
    for item in &mut items {
        if item.text.trim().is_empty()
            && !item.image
            && item.line.is_none()
            && item.rect.area() >= slide_area * BACKGROUND_FRACTION
            && item.source == Source::Slide
        {
            item.is_background = true;
            if !explicit_bg && let Some((color, alpha)) = item.fill {
                paper = paper.blend(color, alpha);
            }
        }
    }

    let content: Vec<&Item> = items
        .iter()
        .filter(|i| !i.is_background && !i.chrome)
        .collect();
    let layouts: Vec<TextLayout> = content.iter().map(|i| layout_text(i)).collect();

    let (gcols, grows) = grid_dims(width, height);
    let mut salience_grid = Grid::new(gcols, grows);
    let mut coverage_grid = Grid::new(gcols, grows);
    for (item, layout) in content.iter().zip(layouts.iter()) {
        paint_item(
            item,
            layout,
            slide_rect,
            paper,
            &mut salience_grid,
            &mut coverage_grid,
        );
    }

    let total_cells = (gcols * grows) as f64;
    let coverage = coverage_grid.covered() as f64 / total_cells;
    let quiet = (0..grows)
        .flat_map(|row| (0..gcols).map(move |col| (col, row)))
        .filter(|(col, row)| salience_grid.value(*col, *row) < QUIET)
        .count() as f64;
    let whitespace = quiet / total_cells;
    let salience = salience_grid.total() / total_cells;

    let (mut sx, mut sy, mut sw) = (0.0, 0.0, 0.0);
    for row in 0..grows {
        for col in 0..gcols {
            let v = salience_grid.value(col, row);
            if v > 0.0 {
                sx += v * (col as f64 + 0.5) / gcols as f64;
                sy += v * (row as f64 + 0.5) / grows as f64;
                sw += v;
            }
        }
    }
    let (balance_x, balance_y) = if sw > 0.0 {
        ((sx / sw - 0.5) * 2.0, (sy / sw - 0.5) * 2.0)
    } else {
        (0.0, 0.0)
    };

    let margins = coverage_grid.bbox(slide_rect).map(|b| Margins {
        l: (b.x - slide_rect.x).max(0.0),
        r: (slide_rect.right() - b.right()).max(0.0),
        t: (b.y - slide_rect.y).max(0.0),
        b: (slide_rect.bottom() - b.bottom()).max(0.0),
    });

    let mut zones = [[0.0f64; 3]; 3];
    for (zi, zone) in zones.iter_mut().enumerate() {
        for (zj, cell) in zone.iter_mut().enumerate() {
            let (c0, c1, r0, r1) = zone_bounds(gcols, grows, zi, zj);
            let mut sum = 0.0;
            let mut n = 0usize;
            for row in r0..r1 {
                for col in c0..c1 {
                    sum += salience_grid.value(col, row);
                    n += 1;
                }
            }
            *cell = if n == 0 { 0.0 } else { sum / n as f64 };
        }
    }

    let largest_void = large_quiet_rect(&salience_grid).map(|(_r, x0, y0, vw, vh)| {
        let cw = width / gcols as f64;
        let ch = height / grows as f64;
        Void {
            share: (vw * vh) as f64 / total_cells,
            rect: Rect {
                x: x0 as f64 * cw,
                y: y0 as f64 * ch,
                w: vw as f64 * cw,
                h: vh as f64 * ch,
            },
            edge: x0 == 0 || y0 == 0 || x0 + vw >= gcols || y0 + vh >= grows,
        }
    });

    let (hcols, hrows) = heat_dims(width, height);
    let heat = salience_grid.downsampled(hcols, hrows);
    let heatmap_lines = (0..hrows)
        .map(|row| {
            let mut line = String::with_capacity(hcols);
            for col in 0..hcols {
                line.push(ramp(heat.value(col, row)));
            }
            line
        })
        .collect();

    let mut words = 0usize;
    let mut chars = 0usize;
    let mut lines = 0usize;
    let mut text_area = 0.0f64;
    let mut zones_words = [[0usize; 3]; 3];
    for layout in &layouts {
        words += layout.words;
        chars += layout.chars;
        lines += layout.lines.len();
        text_area += layout.area;
        for line in &layout.lines {
            let cx = (line.rect.x + line.rect.w / 2.0).clamp(0.0, width.max(0.01));
            let cy = (line.rect.y + line.rect.h / 2.0).clamp(0.0, height.max(0.01));
            let zx = ((cx / width) * 3.0) as usize;
            let zy = ((cy / height) * 3.0) as usize;
            zones_words[zy.min(2)][zx.min(2)] += line.words;
        }
    }
    let content_area = coverage * slide_area;
    let density = if slide_area > 0.0 {
        words as f64 / slide_area
    } else {
        0.0
    };
    let content_density = if content_area > 0.0 {
        words as f64 / content_area
    } else {
        0.0
    };
    let (decoration_share, decoration_count, decorations) = decoration(&content, slide_area);
    let palette = palette(&content, &layouts, paper, slide_area);
    let structure = structure(&content, layout_map(&content, &layouts), paper);

    SlideMeasure {
        slide: name,
        master,
        composition: Composition {
            coverage,
            whitespace,
            largest_void,
            salience,
            balance_x,
            balance_y,
            margins,
            zones,
        },
        content: Content {
            words,
            chars,
            lines,
            text_area,
            content_area,
            density,
            content_density,
            decoration_share,
            decoration_count,
            decorations,
            zones_words,
        },
        structure,
        palette,
        heatmap: Heatmap {
            cols: hcols,
            rows: hrows,
            lines: heatmap_lines,
        },
    }
}

fn zone_bounds(cols: usize, rows: usize, zi: usize, zj: usize) -> (usize, usize, usize, usize) {
    let c0 = zj * cols / 3;
    let c1 = ((zj + 1) * cols / 3).max(c0 + 1);
    let r0 = zi * rows / 3;
    let r1 = ((zi + 1) * rows / 3).max(r0 + 1);
    (c0, c1.min(cols), r0, r1.min(rows))
}

fn ramp(v: f64) -> char {
    if v < 0.01 {
        ' '
    } else if v < 0.05 {
        '.'
    } else if v < 0.12 {
        ':'
    } else if v < 0.20 {
        '-'
    } else if v < 0.32 {
        '='
    } else if v < 0.46 {
        '+'
    } else if v < 0.62 {
        '*'
    } else if v < 0.78 {
        '#'
    } else if v < 0.90 {
        '%'
    } else {
        '@'
    }
}

fn paint_item(
    item: &Item,
    layout: &TextLayout,
    slide: Rect,
    paper: Rgb,
    salience: &mut Grid,
    coverage: &mut Grid,
) {
    let bg = item.fill.map(|(c, a)| paper.blend(c, a)).unwrap_or(paper);
    if let Some((fill, alpha)) = item.fill {
        let blended = paper.blend(fill, alpha);
        let w = visual_weight(contrast(blended, paper));
        salience.rasterize(slide, item.rect, w);
        coverage.rasterize(slide, item.rect, 1.0);
    }
    if let Some((color, width)) = item.line {
        let b = width
            .min(item.rect.w * 0.25)
            .min(item.rect.h * 0.25)
            .max(0.005);
        let w = visual_weight(contrast(color, bg));
        for band in [
            Rect { h: b, ..item.rect },
            Rect {
                y: item.rect.bottom() - b,
                h: b,
                ..item.rect
            },
            Rect { w: b, ..item.rect },
            Rect {
                x: item.rect.right() - b,
                w: b,
                ..item.rect
            },
        ] {
            salience.rasterize(slide, band, w);
            coverage.rasterize(slide, band, 1.0);
        }
    }
    if item.image {
        salience.rasterize(slide, item.rect, 0.55);
        coverage.rasterize(slide, item.rect, 1.0);
        return;
    }
    for line in &layout.lines {
        let w = visual_weight(contrast(line.color, bg));
        salience.rasterize(slide, line.rect, w * 0.9);
        coverage.rasterize(slide, line.rect, 1.0);
    }
}

fn decoration(items: &[&Item], slide_area: f64) -> (f64, usize, Vec<Decoration>) {
    let mut blocks: Vec<&Item> = items
        .iter()
        .copied()
        .filter(|i| {
            if !i.text.trim().is_empty() || i.image || (i.fill.is_none() && i.line.is_none()) {
                return false;
            }
            // A surface that contains text (a card, a layer band) is a
            // content container, not decoration. A thin rule or colour bar
            // with no text is decoration.
            !items
                .iter()
                .any(|other| !other.text.trim().is_empty() && rect_contains(i.rect, other.rect))
        })
        .collect();
    blocks.sort_by(|a, b| {
        b.rect
            .area()
            .partial_cmp(&a.rect.area())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let count = blocks.len();
    let area: f64 = blocks.iter().map(|i| i.rect.area()).sum();
    let share = if slide_area > 0.0 {
        (area / slide_area).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let decorations = blocks
        .into_iter()
        .take(MAX_LIST)
        .map(|i| Decoration {
            label: i.label.clone(),
            rect: i.rect,
            color: i.fill.map(|(c, _)| c).or_else(|| i.line.map(|(c, _)| c)),
        })
        .collect();
    (share, count, decorations)
}

fn rect_contains(outer: Rect, inner: Rect) -> bool {
    inner.x >= outer.x - CONTAIN_TOL
        && inner.y >= outer.y - CONTAIN_TOL
        && inner.right() <= outer.right() + CONTAIN_TOL
        && inner.bottom() <= outer.bottom() + CONTAIN_TOL
}

fn layout_map<'a>(
    items: &[&'a Item],
    layouts: &'a [TextLayout],
) -> Vec<(&'a Item, &'a TextLayout)> {
    items.iter().copied().zip(layouts.iter()).collect()
}

// ---------------------------------------------------------------------------
// Colour palette

fn palette(items: &[&Item], layouts: &[TextLayout], paper: Rgb, slide_area: f64) -> Palette {
    #[derive(Clone, Copy)]
    struct Use {
        color: Rgb,
        weight: f64,
        count: usize,
    }
    let mut uses: BTreeMap<Rgb, Use> = BTreeMap::new();
    let mut add = |color: Rgb, weight: f64| {
        if weight <= 0.0 {
            return;
        }
        uses.entry(color)
            .and_modify(|u| {
                u.weight += weight;
                u.count += 1;
            })
            .or_insert(Use {
                color,
                weight,
                count: 1,
            });
    };
    if slide_area > 0.0 {
        add(paper, slide_area * 0.15);
    }
    for item in items {
        if let Some((c, a)) = item.fill {
            add(paper.blend(c, a), item.rect.area().max(0.01) * a);
        }
        if let Some((c, w)) = item.line {
            add(c, 2.0 * (item.rect.w + item.rect.h) * w.max(0.005));
        }
    }
    let mut contrasts = Vec::new();
    for (item, layout) in items.iter().zip(layouts.iter()) {
        let bg = item.fill.map(|(c, a)| paper.blend(c, a)).unwrap_or(paper);
        for line in &layout.lines {
            add(line.color, (line.rect.area() * 0.5).max(0.001));
            contrasts.push(contrast(line.color, bg));
        }
    }

    let total: f64 = uses.values().map(|u| u.weight).sum();
    let raw_colors = uses.len();
    let mut top_raw: Vec<Use> = uses.values().copied().collect();
    top_raw.sort_by(|a, b| {
        b.weight
            .partial_cmp(&a.weight)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_raw.truncate(5);
    let mut clusters: Vec<(f64, f64, f64, f64, usize, Rgb, f64)> = Vec::new();
    let mut sorted: Vec<Use> = uses.values().copied().collect();
    sorted.sort_by(|a, b| {
        b.weight
            .partial_cmp(&a.weight)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for u in sorted {
        let lab = rgb_to_lab(u.color);
        if let Some(c) = clusters
            .iter_mut()
            .filter(|c| delta_e((c.0, c.1, c.2), lab) < 12.0)
            .min_by(|a, b| {
                delta_e((a.0, a.1, a.2), lab)
                    .partial_cmp(&delta_e((b.0, b.1, b.2), lab))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        {
            c.0 = (c.0 * c.3 + lab.0 * u.weight) / (c.3 + u.weight);
            c.1 = (c.1 * c.3 + lab.1 * u.weight) / (c.3 + u.weight);
            c.2 = (c.2 * c.3 + lab.2 * u.weight) / (c.3 + u.weight);
            c.3 += u.weight;
            c.4 += u.count;
            if u.weight > c.6 {
                c.5 = u.color;
                c.6 = u.weight;
            }
        } else {
            clusters.push((lab.0, lab.1, lab.2, u.weight, u.count, u.color, u.weight));
        }
    }
    clusters.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal));
    let families = clusters.len();
    let mut neutrals = 0usize;
    let mut accents = 0usize;
    for c in &clusters {
        if (c.1.powi(2) + c.2.powi(2)).sqrt() < 12.0 {
            neutrals += 1;
        } else {
            accents += 1;
        }
    }
    let top = top_raw
        .iter()
        .map(|u| ColorShare {
            color: u.color,
            share: if total > 0.0 { u.weight / total } else { 0.0 },
        })
        .collect();
    let one_off = uses
        .values()
        .filter(|u| u.count == 1 && total > 0.0 && u.weight / total < 0.02)
        .map(|u| u.color)
        .take(MAX_LIST)
        .collect();
    let contrast_min = contrasts
        .iter()
        .copied()
        .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let low_contrast_runs = contrasts.iter().filter(|c| **c < 4.5).count();
    Palette {
        families,
        raw_colors,
        neutrals,
        accents,
        top,
        contrast_min,
        low_contrast_runs,
        one_off,
    }
}

// ---------------------------------------------------------------------------
// Structure

fn visual_rect(item: &Item, layout: &TextLayout) -> Rect {
    if item.fill.is_none() && item.line.is_none() && !item.image && !layout.lines.is_empty() {
        let mut r = layout.lines[0].rect;
        for line in &layout.lines[1..] {
            r = r.union(line.rect);
        }
        r
    } else {
        item.rect
    }
}

fn containment(rects: &[Rect]) -> Vec<Option<usize>> {
    let mut parents = vec![None; rects.len()];
    for (i, a) in rects.iter().enumerate() {
        let mut best: Option<(usize, f64)> = None;
        for (j, b) in rects.iter().enumerate() {
            if i == j {
                continue;
            }
            let contains = a.x >= b.x - CONTAIN_TOL
                && a.y >= b.y - CONTAIN_TOL
                && a.right() <= b.right() + CONTAIN_TOL
                && a.bottom() <= b.bottom() + CONTAIN_TOL
                && b.area() > a.area() + 1e-9;
            if contains && best.is_none_or(|(_, area)| b.area() < area) {
                best = Some((j, b.area()));
            }
        }
        parents[i] = best.map(|(j, _)| j);
    }
    parents
}

fn is_ancestor(parents: &[Option<usize>], ancestor: usize, mut node: usize) -> bool {
    while let Some(p) = parents[node] {
        if p == ancestor {
            return true;
        }
        node = p;
    }
    false
}

fn cluster(mut values: Vec<f64>) -> Vec<(f64, usize)> {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut out: Vec<(f64, usize)> = Vec::new();
    for v in values {
        match out.last_mut() {
            Some((pos, n)) if (v - *pos).abs() <= ALIGN_TOL => {
                *pos = (*pos * *n as f64 + v) / (*n as f64 + 1.0);
                *n += 1;
            }
            _ => out.push((v, 1)),
        }
    }
    out
}

fn top_lines(mut lines: Vec<(f64, usize)>) -> Vec<(f64, usize)> {
    lines.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.partial_cmp(&b.0).unwrap()));
    lines.truncate(MAX_LIST);
    lines
}

fn structure(items: &[&Item], layouts: Vec<(&Item, &TextLayout)>, paper: Rgb) -> Structure {
    let vis: Vec<Rect> = layouts
        .iter()
        .map(|(item, layout)| visual_rect(item, layout))
        .collect();
    let parents = containment(&vis);
    let visible_surface = |item: &Item| {
        item.image
            || item.line.is_some()
            || item
                .fill
                .is_some_and(|(c, a)| visual_weight(contrast(paper.blend(c, a), paper)) > 0.02)
    };
    let mut structural: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| visible_surface(item))
        .map(|(i, _)| i)
        .collect();
    if structural.is_empty() {
        structural = (0..items.len()).collect();
    }
    let svis: Vec<Rect> = structural.iter().map(|i| vis[*i]).collect();
    let mut sides: [Vec<(f64, usize)>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    for (r, i) in svis.iter().zip(structural.iter()) {
        sides[0].push((r.x, *i));
        sides[1].push((r.right(), *i));
        sides[2].push((r.y, *i));
        sides[3].push((r.bottom(), *i));
    }
    for s in &mut sides {
        s.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    }

    let mut unaligned = Vec::new();
    for (side, entries) in sides.iter().enumerate() {
        for (idx, (pos, item_idx)) in entries.iter().enumerate() {
            let mut nearest: Option<f64> = None;
            for (other_idx, (other, other_item)) in entries.iter().enumerate() {
                if idx == other_idx || item_idx == other_item {
                    continue;
                }
                let d = (other - pos).abs();
                nearest = Some(nearest.map_or(d, |n| n.min(d)));
            }
            if nearest.unwrap_or(f64::MAX) > ALIGN_TOL {
                unaligned.push(Unaligned {
                    item: items[*item_idx].label.clone(),
                    side: ["left", "right", "top", "bottom"][side].to_string(),
                    at: *pos,
                    nearest,
                });
            }
        }
    }
    let total_edges = structural.len() * 4;
    let unaligned_count = unaligned.len();
    let alignment = if total_edges == 0 {
        0.0
    } else {
        (total_edges - unaligned_count) as f64 / total_edges as f64
    };
    unaligned.sort_by(|a, b| {
        b.nearest
            .unwrap_or(f64::MAX)
            .partial_cmp(&a.nearest.unwrap_or(f64::MAX))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    unaligned.truncate(MAX_LIST);

    let x_lines = top_lines(
        cluster(
            sides[0]
                .iter()
                .chain(sides[1].iter())
                .map(|(v, _)| *v)
                .collect(),
        )
        .into_iter()
        .filter(|(_, n)| *n > 1)
        .collect(),
    );
    let y_lines = top_lines(
        cluster(
            sides[2]
                .iter()
                .chain(sides[3].iter())
                .map(|(v, _)| *v)
                .collect(),
        )
        .into_iter()
        .filter(|(_, n)| *n > 1)
        .collect(),
    );

    let mut near: Vec<NearMiss> = Vec::new();
    let mut seen = BTreeSet::new();
    for (side, entries) in sides.iter().enumerate() {
        for (idx, (pos, item_idx)) in entries.iter().enumerate() {
            let a = vis[*item_idx];
            let mut best: Option<(f64, usize)> = None;
            for (other, other_item) in entries.iter().skip(idx + 1) {
                let b = vis[*other_item];
                let parallel = if side < 2 {
                    y_overlap_ratio(a, b) > 0.5
                } else {
                    x_overlap_ratio(a, b) > 0.5
                };
                let thin = |r: Rect| r.w.min(r.h) < 0.15;
                if !parallel
                    || thin(a)
                    || thin(b)
                    || is_ancestor(&parents, *item_idx, *other_item)
                    || is_ancestor(&parents, *other_item, *item_idx)
                    || nested_label(items[*item_idx], a, items[*other_item], b)
                    || nested_label(items[*other_item], b, items[*item_idx], a)
                {
                    continue;
                }
                let gap = other - pos;
                if gap > ALIGN_TOL && gap <= NEAR_MISS_MAX && best.is_none_or(|(g, _)| gap < g) {
                    best = Some((gap, *other_item));
                }
            }
            if let Some((gap, other_item)) = best {
                let key = if *item_idx < other_item {
                    (*item_idx, other_item)
                } else {
                    (other_item, *item_idx)
                };
                if seen.insert(key) {
                    near.push(NearMiss {
                        a: items[*item_idx].label.clone(),
                        b: items[other_item].label.clone(),
                        gap,
                    });
                }
            }
        }
    }
    near.sort_by(|a, b| {
        a.gap
            .partial_cmp(&b.gap)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let near_miss_count = near.len();
    near.truncate(MAX_LIST);

    let mut vgaps: Vec<f64> = Vec::new();
    let mut hgaps: Vec<f64> = Vec::new();
    let mut groups: BTreeMap<Option<usize>, Vec<usize>> = BTreeMap::new();
    for (i, p) in parents.iter().enumerate() {
        groups.entry(*p).or_default().push(i);
    }
    let mut seen_pairs: BTreeSet<(usize, usize, u8)> = BTreeSet::new();
    for members in groups.values() {
        let members: Vec<usize> = members
            .iter()
            .copied()
            .filter(|i| structural.contains(i))
            .collect();
        for ia in &members {
            let a = vis[*ia];
            let mut nearest_v: Option<(f64, usize)> = None;
            let mut nearest_h: Option<(f64, usize)> = None;
            for ib in &members {
                if *ia == *ib {
                    continue;
                }
                let b = vis[*ib];
                if x_overlap_ratio(a, b) > 0.5 {
                    let gap = b.y - a.bottom();
                    if gap > INK_EPS && nearest_v.is_none_or(|(g, _)| gap < g) {
                        nearest_v = Some((gap, *ib));
                    }
                }
                if y_overlap_ratio(a, b) > 0.5 {
                    let gap = b.x - a.right();
                    if gap > INK_EPS && nearest_h.is_none_or(|(g, _)| gap < g) {
                        nearest_h = Some((gap, *ib));
                    }
                }
            }
            if let Some((gap, ib)) = nearest_v {
                let key = ((*ia).min(ib), (*ia).max(ib), 0);
                if seen_pairs.insert(key) {
                    vgaps.push(gap);
                }
            }
            if let Some((gap, ib)) = nearest_h {
                let key = ((*ia).min(ib), (*ia).max(ib), 1);
                if seen_pairs.insert(key) {
                    hgaps.push(gap);
                }
            }
        }
    }

    let mut collisions = Vec::new();
    let mut collision_count = 0usize;
    let ink_rects: Vec<Vec<Rect>> = layouts
        .iter()
        .map(|(item, layout)| ink_rects(item, layout))
        .collect();
    for i in 0..items.len() {
        for j in (i + 1)..items.len() {
            if is_ancestor(&parents, i, j) || is_ancestor(&parents, j, i) {
                continue;
            }
            // A text-only label centred inside a filled card is part of that
            // card's content surface, not a collision.
            let container = |card: &Item, inside: &Item, inside_rect: Rect| {
                (!inside.text.trim().is_empty() || inside.image)
                    && card.text.trim().is_empty()
                    && card.fill.is_some()
                    && !card.image
                    && point_in_rect(
                        card.rect,
                        inside_rect.x + inside_rect.w / 2.0,
                        inside_rect.y + inside_rect.h / 2.0,
                    )
            };
            if container(items[i], items[j], vis[j]) || container(items[j], items[i], vis[i]) {
                continue;
            }
            if nested_label(items[i], vis[i], items[j], vis[j])
                || nested_label(items[j], vis[j], items[i], vis[i])
            {
                continue;
            }
            let area_a: f64 = ink_rects[i].iter().map(|r| r.area()).sum();
            let area_b: f64 = ink_rects[j].iter().map(|r| r.area()).sum();
            if area_a <= 0.0 || area_b <= 0.0 {
                continue;
            }
            let inter = ink_rects[i]
                .iter()
                .flat_map(|r| ink_rects[j].iter().map(move |s| overlap_area(*r, *s)))
                .sum::<f64>();
            if inter <= 0.0 || inter / area_a.min(area_b) <= OVERLAP_FRACTION {
                continue;
            }
            // Rules, dividers, and bracket strokes meet at T-junctions by
            // design. They are ink, but not collisions.
            let thin = |r: Rect| r.w.min(r.h) < 0.15;
            if thin(vis[i]) || thin(vis[j]) {
                continue;
            }
            if items[i].shape == "chevron"
                && items[j].shape == "chevron"
                && x_overlap_ratio(vis[i], vis[j]).min(1.0) * vis[i].w.min(vis[j].w) <= 1.5
            {
                continue;
            }
            collision_count += 1;
            if collisions.len() < MAX_LIST {
                let cx = (vis[i].x.max(vis[j].x) + vis[i].right().min(vis[j].right())) / 2.0;
                let cy = (vis[i].y.max(vis[j].y) + vis[i].bottom().min(vis[j].bottom())) / 2.0;
                collisions.push(format!(
                    "{} x {} @ ({:.2}, {:.2})",
                    items[i].label, items[j].label, cx, cy
                ));
            }
        }
    }

    Structure {
        alignment,
        unaligned_count,
        unaligned,
        x_lines,
        y_lines,
        near_miss_count,
        near_miss: near,
        vertical: gap_stats(vgaps),
        horizontal: gap_stats(hgaps),
        collision_count,
        collisions,
    }
}

fn point_in_rect(r: Rect, x: f64, y: f64) -> bool {
    x >= r.x && x <= r.right() && y >= r.y && y <= r.bottom()
}

fn nested_label(big: &Item, big_rect: Rect, small: &Item, small_rect: Rect) -> bool {
    if small.text.trim().is_empty() || big_rect.area() <= 0.0 || (big.fill.is_none() && !big.image)
    {
        return false;
    }
    let small_area = small_rect.area();
    small_area / big_rect.area() < 0.35 && overlap_area(big_rect, small_rect) > small_area * 0.2
}

fn ink_rects(item: &Item, layout: &TextLayout) -> Vec<Rect> {
    let mut out = Vec::new();
    if let Some((_, _)) = item.fill {
        out.push(item.rect);
    }
    if let Some((_, width)) = item.line {
        let b = width
            .min(item.rect.w * 0.25)
            .min(item.rect.h * 0.25)
            .max(0.005);
        out.push(Rect { h: b, ..item.rect });
        out.push(Rect {
            y: item.rect.bottom() - b,
            h: b,
            ..item.rect
        });
        out.push(Rect { w: b, ..item.rect });
        out.push(Rect {
            x: item.rect.right() - b,
            w: b,
            ..item.rect
        });
    }
    if item.image {
        out.push(item.rect);
    }
    for line in &layout.lines {
        out.push(line.rect);
    }
    if out.is_empty() && !item.text.trim().is_empty() {
        out.push(item.rect);
    }
    out
}

fn x_overlap_ratio(a: Rect, b: Rect) -> f64 {
    let overlap = (a.right().min(b.right()) - a.x.max(b.x)).max(0.0);
    if a.w.min(b.w) <= 0.0 {
        0.0
    } else {
        overlap / a.w.min(b.w)
    }
}

fn y_overlap_ratio(a: Rect, b: Rect) -> f64 {
    let overlap = (a.bottom().min(b.bottom()) - a.y.max(b.y)).max(0.0);
    if a.h.min(b.h) <= 0.0 {
        0.0
    } else {
        overlap / a.h.min(b.h)
    }
}

fn gap_stats(mut gaps: Vec<f64>) -> GapStats {
    if gaps.is_empty() {
        return GapStats::default();
    }
    gaps.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = gaps.len();
    let median = if n.is_multiple_of(2) {
        (gaps[n / 2 - 1] + gaps[n / 2]) / 2.0
    } else {
        gaps[n / 2]
    };
    let mean = gaps.iter().sum::<f64>() / n as f64;
    let cv = if mean > 0.0 {
        let var = gaps.iter().map(|g| (g - mean).powi(2)).sum::<f64>() / n as f64;
        Some(var.sqrt() / mean)
    } else {
        None
    };
    let tolerance = (median.abs() * 0.1).max(0.02);
    let even = gaps
        .iter()
        .filter(|g| (*g - median).abs() <= tolerance)
        .count();
    GapStats {
        median: Some(median),
        even,
        total: n,
        cv,
    }
}

// ---------------------------------------------------------------------------
// Rendering

fn cm(v: f64) -> f64 {
    v * CM_PER_IN
}

fn cm2(v: f64) -> f64 {
    v * CM2_PER_IN2
}

fn per_cm2(v: f64) -> f64 {
    v / CM2_PER_IN2
}

fn round(v: f64, places: u32) -> f64 {
    let factor = 10f64.powi(places as i32);
    (v * factor).round() / factor
}

fn clean(v: f64) -> f64 {
    if v.abs() < 0.0005 { 0.0 } else { v }
}

fn f2(v: f64) -> String {
    format!("{:.2}", clean(v))
}

fn f1(v: f64) -> String {
    format!("{:.1}", clean(v))
}

fn f3(v: f64) -> String {
    format!("{:.3}", clean(v))
}

fn pct(v: f64) -> String {
    let n = (clean(v) * 100.0).round() as i64;
    if n == 0 {
        "0%".into()
    } else {
        format!("{n:+}%")
    }
}

fn share(v: f64) -> String {
    let p = (v * 100.0).clamp(0.0, 100.0);
    if p < 0.05 {
        "0%".into()
    } else if p < 0.5 {
        "<1%".into()
    } else {
        format!("{p:.0}%")
    }
}

struct Md {
    lines: Vec<String>,
}

impl Md {
    fn new() -> Self {
        Self { lines: Vec::new() }
    }

    fn line(&mut self, text: impl Into<String>) {
        let raw = text.into();
        if raw.is_empty() {
            self.lines.push(String::new());
            return;
        }
        for part in raw.split('\n') {
            let mut line = part.replace('\r', "").replace('\t', "    ");
            while line.ends_with(' ') || line.ends_with('\t') {
                line.pop();
            }
            self.lines.push(line);
        }
    }

    fn blank(&mut self) {
        if self.lines.last().is_some_and(|l| !l.is_empty()) {
            self.lines.push(String::new());
        }
    }

    fn fence(&mut self, lang: &str, body: &[String]) {
        self.blank();
        self.line(format!("```{lang}"));
        for line in body {
            self.line(line.clone());
        }
        self.line("```");
        self.blank();
    }

    fn table(&mut self, header: &[&str], align: &[&str], rows: &[Vec<String>]) {
        self.blank();
        self.line(format!("| {} |", header.join(" | ")));
        let sep = align
            .iter()
            .map(|a| match *a {
                "r" => "---:",
                "c" => ":---:",
                _ => ":---",
            })
            .collect::<Vec<_>>()
            .join(" | ");
        self.line(format!("| {sep} |"));
        for row in rows {
            self.line(format!("| {} |", row.join(" | ")));
        }
        self.blank();
    }

    fn finish(mut self) -> String {
        while self.lines.last().is_some_and(|l| l.is_empty()) {
            self.lines.pop();
        }
        let mut out = self.lines.join("\n");
        out.push('\n');
        out
    }
}

impl Measurement {
    /// Render the deck measurement as Markdown.
    pub fn to_markdown(&self) -> String {
        let mut md = Md::new();
        let title = if self.title.trim().is_empty() {
            "deck"
        } else {
            self.title.trim()
        };
        md.line(format!("# gwen measure: {title}"));
        md.blank();
        md.line(format!("- slides: {}", self.total_slides));
        if let Some(pages) = &self.page {
            md.line(format!("- page: {}", PageSelection::format(pages)));
        }
        md.line(format!(
            "- canvas: {} x {} cm",
            f2(cm(self.width)),
            f2(cm(self.height))
        ));
        md.line(format!(
            "- deck: whitespace {:.2} avg, salience {:.2} avg, words {:.0} avg",
            self.deck.whitespace, self.deck.salience, self.deck.words
        ));

        for slide in &self.slides {
            md.blank();
            let heading = match &slide.master {
                Some(master) => format!("## {} (master {})", slide.slide, master),
                None => format!("## {}", slide.slide),
            };
            md.line(heading);
            md.blank();
            md.line("### Composition");
            md.blank();
            md.line(format!("- coverage: {:.2}", slide.composition.coverage));
            md.line(format!("- whitespace: {:.2}", slide.composition.whitespace));
            match slide.composition.largest_void {
                Some(v) => md.line(format!(
                    "- largest_void: {} @ ({}, {}) {}x{} cm [{}]",
                    f2(v.share),
                    f2(cm(v.rect.x)),
                    f2(cm(v.rect.y)),
                    f2(cm(v.rect.w)),
                    f2(cm(v.rect.h)),
                    if v.edge { "edge" } else { "interior" }
                )),
                None => md.line("- largest_void: none"),
            }
            md.line(format!("- salience: {:.2}", slide.composition.salience));
            md.line(format!(
                "- balance: x {}, y {}",
                pct(slide.composition.balance_x),
                pct(slide.composition.balance_y)
            ));
            match slide.composition.margins {
                Some(m) => md.line(format!(
                    "- margins (cm): l {}  r {}  t {}  b {}",
                    f2(cm(m.l)),
                    f2(cm(m.r)),
                    f2(cm(m.t)),
                    f2(cm(m.b))
                )),
                None => md.line("- margins (cm): -"),
            }

            md.blank();
            md.line("**zones (salience)**");
            let rows = (0..3)
                .map(|i| {
                    vec![
                        ["top", "mid", "bottom"][i].to_string(),
                        f2(slide.composition.zones[i][0]),
                        f2(slide.composition.zones[i][1]),
                        f2(slide.composition.zones[i][2]),
                    ]
                })
                .collect::<Vec<_>>();
            md.table(
                &["zone", "left", "center", "right"],
                &["l", "r", "r", "r"],
                &rows,
            );

            let mut map = vec![format!("salience map (cm; chrome excluded)")];
            map.push(ruler(self.width, slide.heatmap.cols));
            for (i, line) in slide.heatmap.lines.iter().enumerate() {
                map.push(format!("{i:>2} | {line} |"));
            }
            map.push(
                "legend: ' '=0  .<5%  :<15%  -<25%  =<35%  +<50%  *<65%  #<80%  %<90%  @>=90%"
                    .into(),
            );
            md.fence("text", &map);

            md.blank();
            md.line("### Content");
            md.blank();
            md.line(format!("- words: {}", slide.content.words));
            md.line(format!("- chars: {}", slide.content.chars));
            md.line(format!("- lines: {}", slide.content.lines));
            md.line(format!(
                "- text_area: {} cm²",
                f1(cm2(slide.content.text_area))
            ));
            md.line(format!(
                "- content_area: {} cm²",
                f1(cm2(slide.content.content_area))
            ));
            md.line(format!(
                "- density: {} words/cm² (slide)",
                f3(per_cm2(slide.content.density))
            ));
            md.line(format!(
                "- content_density: {} words/cm² (content_area)",
                f3(per_cm2(slide.content.content_density))
            ));
            if slide.content.decorations.is_empty() {
                md.line(format!(
                    "- decoration: {} (0 blocks)",
                    share(slide.content.decoration_share)
                ));
            } else {
                md.line(format!(
                    "- decoration: {} ({} blocks)",
                    share(slide.content.decoration_share),
                    slide.content.decoration_count
                ));
                for block in &slide.content.decorations {
                    md.line(format!(
                        "  - {} {}x{} cm {} (no text)",
                        block.label,
                        f2(cm(block.rect.w)),
                        f2(cm(block.rect.h)),
                        block
                            .color
                            .map(|c| c.hex())
                            .unwrap_or_else(|| "none".into())
                    ));
                }
            }

            md.blank();
            md.line("**zones (words)**");
            let rows = (0..3)
                .map(|i| {
                    vec![
                        ["top", "mid", "bottom"][i].to_string(),
                        slide.content.zones_words[i][0].to_string(),
                        slide.content.zones_words[i][1].to_string(),
                        slide.content.zones_words[i][2].to_string(),
                    ]
                })
                .collect::<Vec<_>>();
            md.table(
                &["zone", "left", "center", "right"],
                &["l", "r", "r", "r"],
                &rows,
            );

            md.blank();
            md.line("### Structure");
            md.blank();
            md.line(format!(
                "- alignment: {:.2} ({} edges unaligned)",
                slide.structure.alignment, slide.structure.unaligned_count
            ));
            if !slide.structure.x_lines.is_empty() {
                md.line(format!(
                    "  - x-lines: {}",
                    slide
                        .structure
                        .x_lines
                        .iter()
                        .map(|(v, n)| format!("{}x{n}", f2(cm(*v))))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if !slide.structure.y_lines.is_empty() {
                md.line(format!(
                    "  - y-lines: {}",
                    slide
                        .structure
                        .y_lines
                        .iter()
                        .map(|(v, n)| format!("{}x{n}", f2(cm(*v))))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if slide.structure.unaligned.is_empty() {
                md.line("- unaligned: 0");
            } else {
                md.line(format!(
                    "- unaligned: {} (showing {})",
                    slide.structure.unaligned_count,
                    slide.structure.unaligned.len()
                ));
                for u in &slide.structure.unaligned {
                    md.line(format!(
                        "  - {} {} @ {} (nearest {})",
                        u.item,
                        u.side,
                        f2(cm(u.at)),
                        u.nearest.map(|n| f2(cm(n))).unwrap_or_else(|| "-".into())
                    ));
                }
            }
            md.line(format!("- near_miss: {}", slide.structure.near_miss_count));
            for n in &slide.structure.near_miss {
                md.line(format!("  - {} vs {} ({})", n.a, n.b, f2(cm(n.gap))));
            }
            md.line("- gaps:");
            for (label, stats) in [
                ("vertical", &slide.structure.vertical),
                ("horizontal", &slide.structure.horizontal),
            ] {
                match stats.median {
                    Some(median) => md.line(format!(
                        "  - {label}: median {} cm, even {}/{}, cv {}",
                        f2(cm(median)),
                        stats.even,
                        stats.total,
                        stats
                            .cv
                            .map(|v| format!("{v:.2}"))
                            .unwrap_or_else(|| "-".into())
                    )),
                    None => md.line(format!("  - {label}: none")),
                }
            }
            if slide.structure.collisions.is_empty() {
                md.line("- collisions: 0");
            } else {
                md.line(format!("- collisions: {}", slide.structure.collision_count));
                for c in &slide.structure.collisions {
                    md.line(format!("  - {c}"));
                }
            }

            md.blank();
            md.line("### Palette");
            md.blank();
            md.line(format!(
                "- families: {} (from {} raw colors)",
                slide.palette.families, slide.palette.raw_colors
            ));
            md.line(format!(
                "- neutrals: {}, accents: {}",
                slide.palette.neutrals, slide.palette.accents
            ));
            md.line(format!(
                "- top: {}",
                if slide.palette.top.is_empty() {
                    "none".into()
                } else {
                    slide
                        .palette
                        .top
                        .iter()
                        .map(|c| format!("{} {}", c.color.hex(), share(c.share)))
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            ));
            match slide.palette.contrast_min {
                Some(c) => md.line(format!(
                    "- contrast: min {:.1}, {} low-contrast run{}",
                    c,
                    slide.palette.low_contrast_runs,
                    if slide.palette.low_contrast_runs == 1 {
                        ""
                    } else {
                        "s"
                    }
                )),
                None => md.line("- contrast: -"),
            }
            md.line(format!(
                "- one_off: {}",
                if slide.palette.one_off.is_empty() {
                    "none".into()
                } else {
                    slide
                        .palette
                        .one_off
                        .iter()
                        .map(|c| c.hex())
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            ));
        }

        md.finish()
    }

    /// Render the deck measurement as JSON. All lengths are centimetres.
    pub fn to_json(&self) -> String {
        let slides: Vec<Value> = self
            .slides
            .iter()
            .map(|s| {
                let void = s.composition.largest_void.map(|v| {
                    serde_json::json!({
                        "share": round(v.share, 3),
                        "x_cm": round(cm(v.rect.x), 2),
                        "y_cm": round(cm(v.rect.y), 2),
                        "w_cm": round(cm(v.rect.w), 2),
                        "h_cm": round(cm(v.rect.h), 2),
                        "edge": v.edge,
                    })
                });
                let margins = s.composition.margins.map(|m| {
                    serde_json::json!({
                        "l_cm": round(cm(m.l), 2),
                        "r_cm": round(cm(m.r), 2),
                        "t_cm": round(cm(m.t), 2),
                        "b_cm": round(cm(m.b), 2),
                    })
                });
                let x_lines: Vec<Value> = s
                    .structure
                    .x_lines
                    .iter()
                    .map(|(pos, n)| serde_json::json!({"cm": round(cm(*pos), 2), "count": n}))
                    .collect();
                let y_lines: Vec<Value> = s
                    .structure
                    .y_lines
                    .iter()
                    .map(|(pos, n)| serde_json::json!({"cm": round(cm(*pos), 2), "count": n}))
                    .collect();
                let unaligned: Vec<Value> = s
                    .structure
                    .unaligned
                    .iter()
                    .map(|u| {
                        serde_json::json!({
                            "item": u.item,
                            "side": u.side,
                            "at_cm": round(cm(u.at), 2),
                            "nearest_cm": u.nearest.map(|n| round(cm(n), 2)),
                        })
                    })
                    .collect();
                let near: Vec<Value> = s
                    .structure
                    .near_miss
                    .iter()
                    .map(|n| {
                        serde_json::json!({
                            "a": n.a, "b": n.b, "gap_cm": round(cm(n.gap), 2)
                        })
                    })
                    .collect();
                let gaps = |g: GapStats| {
                    serde_json::json!({
                        "median_cm": g.median.map(|m| round(cm(m), 2)),
                        "even": g.even,
                        "total": g.total,
                        "cv": g.cv.map(|v| round(v, 2)),
                    })
                };
                let decorations: Vec<Value> = s
                    .content
                    .decorations
                    .iter()
                    .map(|b| {
                        serde_json::json!({
                            "label": b.label,
                            "w_cm": round(cm(b.rect.w), 2),
                            "h_cm": round(cm(b.rect.h), 2),
                            "color": b.color.map(|c| c.hex()),
                        })
                    })
                    .collect();
                let top: Vec<Value> = s
                    .palette
                    .top
                    .iter()
                    .map(|c| serde_json::json!({"hex": c.color.hex(), "share": round(c.share, 3)}))
                    .collect();
                serde_json::json!({
                    "slide": s.slide,
                    "master": s.master,
                    "composition": {
                        "coverage": round(s.composition.coverage, 3),
                        "whitespace": round(s.composition.whitespace, 3),
                        "largest_void": void,
                        "salience": round(s.composition.salience, 3),
                        "balance": {
                            "x": round(s.composition.balance_x, 3),
                            "y": round(s.composition.balance_y, 3),
                        },
                        "margins_cm": margins,
                        "zones_salience": s
                            .composition
                            .zones
                            .iter()
                            .map(|row| row.iter().map(|v| round(*v, 3)).collect::<Vec<_>>())
                            .collect::<Vec<_>>(),
                    },
                    "content": {
                        "words": s.content.words,
                        "chars": s.content.chars,
                        "lines": s.content.lines,
                        "text_area_cm2": round(cm2(s.content.text_area), 1),
                        "content_area_cm2": round(cm2(s.content.content_area), 1),
                        "density_words_per_cm2": round(per_cm2(s.content.density), 3),
                        "content_density_words_per_cm2": round(
                            per_cm2(s.content.content_density),
                            3,
                        ),
                        "decoration": {
                            "share": round(s.content.decoration_share, 3),
                            "count": s.content.decoration_count,
                            "blocks": decorations,
                        },
                        "zones_words": s.content.zones_words,
                    },
                    "structure": {
                        "alignment": round(s.structure.alignment, 3),
                        "unaligned_count": s.structure.unaligned_count,
                        "x_lines": x_lines,
                        "y_lines": y_lines,
                        "unaligned": unaligned,
                        "near_miss_count": s.structure.near_miss_count,
                        "near_miss": near,
                        "gaps": {
                            "vertical": gaps(s.structure.vertical),
                            "horizontal": gaps(s.structure.horizontal),
                        },
                        "collision_count": s.structure.collision_count,
                        "collisions": s.structure.collisions,
                    },
                    "palette": {
                        "families": s.palette.families,
                        "raw_colors": s.palette.raw_colors,
                        "neutrals": s.palette.neutrals,
                        "accents": s.palette.accents,
                        "top": top,
                        "contrast": {
                            "min": s.palette.contrast_min.map(|c| round(c, 1)),
                            "low_contrast_runs": s.palette.low_contrast_runs,
                        },
                        "one_off": s.palette.one_off.iter().map(|c| c.hex()).collect::<Vec<_>>(),
                    },
                    "heatmap": {
                        "cols": s.heatmap.cols,
                        "rows": s.heatmap.rows,
                        "cell_w_cm": round(cm(self.width / s.heatmap.cols as f64), 2),
                        "cell_h_cm": round(cm(self.height / s.heatmap.rows as f64), 2),
                        "rows_text": s.heatmap.lines,
                    },
                })
            })
            .collect();
        let value = serde_json::json!({
            "title": self.title,
            "units": "cm",
            "page": self.page,
            "slides_total": self.total_slides,
            "width_cm": round(cm(self.width), 2),
            "height_cm": round(cm(self.height), 2),
            "deck": {
                "whitespace_avg": round(self.deck.whitespace, 3),
                "salience_avg": round(self.deck.salience, 3),
                "words_avg": round(self.deck.words, 1),
                "coverage_avg": round(self.deck.coverage, 3),
            },
            "slides": slides,
        });
        serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".into())
    }
}

fn ruler(width: f64, cols: usize) -> String {
    let mut chars = vec![' '; cols];
    let mut step = 0.0;
    while step <= cm(width) + 0.01 {
        let idx = ((step / cm(width).max(0.01)) * cols as f64).round() as usize;
        let label = format!("{step:.0}");
        for (offset, c) in label.chars().enumerate() {
            if idx + offset < cols {
                chars[idx + offset] = c;
            }
        }
        step += 5.0;
    }
    let text: String = chars.into_iter().collect();
    format!("     {} cm", text.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn spec(slides: Vec<Value>) -> Value {
        json!({
            "width": 13.333,
            "height": 7.5,
            "masters": [],
            "sections": [{"title": "s", "slides": slides}],
        })
    }

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Value {
        json!({
            "kind": "shape", "type": "rect",
            "x": x, "y": y, "w": w, "h": h,
            "fill": { "color": "C7000A" },
        })
    }

    fn text(x: f64, y: f64, w: f64, h: f64, t: &str, font: f64) -> Value {
        json!({
            "kind": "text", "x": x, "y": y, "w": w, "h": h,
            "fontSize": font, "runs": [{"text": t}],
        })
    }

    fn first(slides: Vec<Value>) -> SlideMeasure {
        let m = measure_spec(&spec(slides), String::new());
        m.slides.into_iter().next().expect("one slide")
    }

    #[test]
    fn empty_slide_is_fully_empty() {
        let s = first(vec![json!({"file": "a.toml", "shapes": []})]);
        assert_eq!(s.composition.coverage, 0.0);
        assert_eq!(s.composition.whitespace, 1.0);
        assert_eq!(s.content.words, 0);
        let void = s
            .composition
            .largest_void
            .expect("the whole slide is quiet");
        assert!(void.share > 0.99, "largest void is the slide: {void:?}");
    }

    #[test]
    fn full_bleed_rect_is_background_not_content() {
        let s = first(vec![
            json!({"file": "a.toml", "shapes": [rect(0.0, 0.0, 13.333, 7.5)]}),
        ]);
        assert_eq!(s.composition.coverage, 0.0, "bg rect is paper");
        assert_eq!(s.content.words, 0);
    }

    #[test]
    fn large_fill_has_salience_but_no_information() {
        let s = first(vec![
            json!({"file": "a.toml", "shapes": [rect(0.5, 0.5, 8.0, 6.0)]}),
        ]);
        assert!(s.composition.salience > 0.2, "large red fill is salient");
        assert_eq!(s.content.words, 0, "but carries no words");
        assert!(s.content.decoration_share > 0.3);
    }

    #[test]
    fn small_text_has_information_but_little_salience() {
        let large = first(vec![
            json!({"file": "a.toml", "shapes": [rect(0.5, 0.5, 8.0, 6.0)]}),
        ]);
        let small = first(vec![
            json!({"file": "a.toml", "shapes": [text(0.5, 0.5, 1.2, 0.35, "Hi", 10.0)]}),
        ]);
        assert!(small.composition.salience < large.composition.salience);
        assert_eq!(small.content.words, 1);
        assert!(small.content.text_area > 0.0);
    }

    #[test]
    fn left_heavy_content_moves_balance_and_margin() {
        let s = first(vec![json!({
            "file": "a.toml",
            "shapes": [
                rect(0.3, 0.5, 4.0, 1.0),
                rect(0.3, 2.0, 4.0, 1.0),
                rect(0.3, 3.5, 4.0, 1.5),
            ],
        })]);
        assert!(s.composition.balance_x < -0.3, "content leans left");
        assert!(s.composition.margins.expect("margins").r > 6.0);
    }

    #[test]
    fn overlapping_siblings_are_reported() {
        let s = first(vec![json!({
            "file": "a.toml",
            "shapes": [
                rect(1.0, 1.0, 3.0, 2.0),
                rect(2.0, 1.5, 3.0, 2.0),
            ],
        })]);
        assert_eq!(s.structure.collision_count, 1);
    }

    #[test]
    fn placeholder_uses_master_rect_and_style() {
        let mut s = spec(vec![json!({
            "file": "a.toml",
            "master": "m",
            "shapes": [{
                "kind": "text", "x": 0.0, "y": 0.0, "w": 0.0, "h": 0.0,
                "placeholder": "title",
                "runs": [{"text": "A title"}],
            }],
        })]);
        s["masters"] = json!([{
            "name": "m",
            "background": null,
            "objects": [{
                "type": "placeholder",
                "x": 1.0, "y": 1.0, "w": 8.0, "h": 1.0,
                "options": {"name": "title", "fontSize": 24, "color": "C7000A", "bold": true},
                "runs": [{"text": "TODO"}],
            }],
        }]);
        let m = measure_spec(&s, String::new());
        let slide = &m.slides[0];
        assert_eq!(slide.content.words, 2);
        let margins = slide.composition.margins.expect("margins");
        assert!((margins.l - 1.0).abs() < 0.1, "left margin {margins:?}");
        assert!(slide.composition.coverage > 0.0);
    }

    #[test]
    fn chevrons_interlocking_are_not_collisions() {
        let s = first(vec![json!({
            "file": "a.toml",
            "shapes": [
                {"kind":"text","shape":"chevron","x":0.5,"y":1.0,"w":2.8,"h":1.0,
                 "fill":{"color":"A6A6A6"},"runs":[{"text":"A"}]},
                {"kind":"text","shape":"chevron","x":2.9,"y":1.0,"w":2.8,"h":1.0,
                 "fill":{"color":"2F5597"},"runs":[{"text":"B"}]},
            ],
        })]);
        assert_eq!(s.structure.collision_count, 0);
    }

    #[test]
    fn page_filter_keeps_deck_average() {
        let m = measure_spec(
            &spec(vec![
                json!({"file": "a.toml", "shapes": [rect(0.3, 0.5, 4.0, 1.0)]}),
                json!({"file": "b.toml", "shapes": [text(1.0, 1.0, 5.0, 1.0, "hello", 20.0)]}),
            ]),
            "deck".into(),
        );
        assert_eq!(m.total_slides, 2);
        assert_eq!(m.slides.len(), 2);
        assert!(m.deck.words > 0.0);
    }

    #[test]
    fn markdown_is_well_formed() {
        let m = measure_spec(
            &spec(vec![
                json!({"file": "a.toml", "shapes": [rect(0.3, 0.5, 4.0, 1.0)]}),
                json!({"file": "b.toml", "shapes": [text(1.0, 1.0, 5.0, 1.0, "hello world", 20.0)]}),
            ]),
            "deck".into(),
        );
        let md = m.to_markdown();
        assert!(
            md.ends_with('\n') && !md.ends_with("\n\n"),
            "single trailing newline"
        );
        assert!(!md.contains('\r'), "no CR");
        assert!(!md.contains('\t'), "no tabs");
        let mut blank = 0usize;
        let mut fence_count = 0usize;
        for line in md.lines() {
            assert!(
                !line.ends_with(' ') && !line.ends_with('\t'),
                "no trailing whitespace: {line:?}"
            );
            if line.is_empty() {
                blank += 1;
                assert!(blank <= 1, "no double blank lines");
            } else {
                blank = 0;
            }
            if line.starts_with("```") {
                fence_count += 1;
            }
        }
        assert_eq!(fence_count % 2, 0, "balanced fences");
        let json: Value = serde_json::from_str(&m.to_json()).expect("measurement JSON parses");
        assert_eq!(json["units"], "cm");
        assert_eq!(json["slides"].as_array().map(|a| a.len()), Some(2));
    }

    #[test]
    fn near_miss_reports_close_but_not_shared_edges() {
        let s = first(vec![json!({
            "file": "a.toml",
            "shapes": [
                rect(1.0, 1.0, 2.0, 1.0),
                rect(2.0, 1.4, 1.2, 1.0),
            ],
        })]);
        assert!(
            s.structure.near_miss_count >= 1,
            "right edges 3.0 and 3.2 should be a near miss: {:?}",
            s.structure.near_miss
        );
    }

    #[test]
    fn gaps_measure_nearest_neighbour_spacing() {
        let s = first(vec![json!({
            "file": "a.toml",
            "shapes": [
                rect(1.0, 1.0, 2.0, 1.0),
                rect(1.0, 2.5, 2.0, 1.0),
            ],
        })]);
        let median = s.structure.vertical.median.expect("vertical gap");
        assert!((median - 0.5).abs() < 0.01, "gap is 0.5in: {median}");
    }
}
