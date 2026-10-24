//! Resolve a deck spec into the flat, renderable scene the preview painter
//! consumes.
//!
//! Everything that decides *what* a slide looks like lives here: master chrome,
//! placeholder inheritance (slide > master placeholder > pptxgenjs text
//! styles), theme colors and font roles, bullets, margins, effects and image
//! sizing. `textlayout` and `svg` only place and paint the result.
//!
//! The scene is built from the same spec `build` hands to pptxgenjs, so the
//! preview follows the TOML's geometry rather than re-reading the generated
//! `.pptx`.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

use crate::color::Color;
use crate::defaults::{Defaults, LevelStyle};
use crate::media;
use crate::presets::{self, Curve, GeomOpts, Point};

const PT_PER_IN: f64 = 72.0;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn right(&self) -> f64 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }
    pub fn inset(&self, m: Margins) -> Rect {
        Rect {
            x: self.x + m.left,
            y: self.y + m.top,
            w: (self.w - m.left - m.right).max(0.0),
            h: (self.h - m.top - m.bottom).max(0.0),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Margins {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

impl Margins {
    /// PowerPoint's default text insets when none are written.
    pub fn default_insets() -> Self {
        Margins {
            top: 0.05,
            right: 0.1,
            bottom: 0.05,
            left: 0.1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
    Justify,
}

impl Align {
    fn parse(value: Option<&str>) -> Option<Self> {
        match value? {
            "left" | "l" => Some(Align::Left),
            "center" | "ctr" => Some(Align::Center),
            "right" | "r" => Some(Align::Right),
            "justify" | "just" => Some(Align::Justify),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Valign {
    Top,
    Middle,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    None,
    Shrink,
    Resize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextDirection {
    Horz,
    Vert,
    Vert270,
    WordArtVert,
}

#[derive(Debug, Clone)]
pub enum Background {
    None,
    Color(Color),
    Image { data: String, transparency: f64 },
}

#[derive(Debug, Clone)]
pub struct Scene {
    pub width: f64,
    pub height: f64,
    pub background: Background,
    pub items: Vec<Item>,
    /// 1-based slide number.
    pub page: usize,
    /// Source slide file, relative to `<deck>/slides`.
    pub file: String,
    /// Features drawn approximately, reported to stderr.
    pub approximations: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Item {
    pub rect: Rect,
    pub rotate: f64,
    pub flip_h: bool,
    pub flip_v: bool,
    pub fill: Option<Color>,
    pub line: Option<Stroke>,
    pub shadow: Option<Shadow>,
    pub even_odd: bool,
    pub kind: ItemKind,
}

#[derive(Debug, Clone)]
pub enum ItemKind {
    Shape {
        preset: String,
        geom: GeomOpts,
    },
    Text(Box<TextBlock>),
    Image {
        data: String,
        sizing: String,
        crop: Option<Rect>,
        ellipse: bool,
        transparency: f64,
    },
}

#[derive(Debug, Clone)]
pub struct Stroke {
    pub color: Color,
    pub width: f64,
    pub dash: Option<String>,
    pub head: Option<String>,
    pub tail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShadowKind {
    Outer,
    Inner,
}

#[derive(Debug, Clone)]
pub struct Shadow {
    pub kind: ShadowKind,
    pub color: Color,
    pub opacity: f64,
    /// Points.
    pub blur: f64,
    /// Degrees, clockwise from 12 o'clock in pptxgenjs (270 is down-right).
    pub angle: f64,
    /// Points.
    pub distance: f64,
    pub rotate_with_shape: bool,
}

#[derive(Debug, Clone)]
pub struct Glow {
    pub color: Color,
    pub opacity: f64,
    /// Points.
    pub size: f64,
}

#[derive(Debug, Clone)]
pub struct TextBlock {
    pub margin: Margins,
    pub align: Align,
    pub valign: Valign,
    pub wrap: bool,
    pub fit: Fit,
    pub direction: TextDirection,
    pub rtl: bool,
    pub runs: Vec<Run>,
    pub line_spacing: Option<f64>,
    pub line_spacing_multiple: Option<f64>,
    pub para_space_before: f64,
    pub para_space_after: f64,
    pub tab_stops: Vec<f64>,
}

#[derive(Debug, Clone)]
pub struct Run {
    pub text: String,
    pub font: String,
    /// Points.
    pub size: f64,
    pub bold: bool,
    pub italic: bool,
    pub underline: Option<String>,
    pub strike: bool,
    pub color: Color,
    pub highlight: Option<Color>,
    pub alpha: f64,
    /// Points.
    pub char_spacing: f64,
    /// Percent of the font size (positive raises, negative lowers).
    pub baseline: f64,
    pub bullet: Option<Bullet>,
    pub indent_level: usize,
    pub break_line: bool,
    pub soft_break_before: bool,
    pub line_spacing: Option<f64>,
    pub line_spacing_multiple: Option<f64>,
    pub para_space_before: f64,
    pub para_space_after: f64,
    pub hyperlink: bool,
    pub outline: Option<TextOutline>,
    pub glow: Option<Glow>,
}

#[derive(Debug, Clone, Copy)]
pub struct TextOutline {
    pub color: Color,
    /// Points.
    pub size: f64,
}

#[derive(Debug, Clone)]
pub enum Bullet {
    Char {
        text: String,
        /// Points; pptxgenjs defaults to 27pt.
        indent_pt: f64,
    },
    Number {
        style: String,
        start: usize,
        /// Points; pptxgenjs defaults to 27pt.
        indent_pt: f64,
    },
}

impl Bullet {
    pub fn indent_in(&self) -> f64 {
        let pt = match self {
            Bullet::Char { indent_pt, .. } | Bullet::Number { indent_pt, .. } => *indent_pt,
        };
        pt / PT_PER_IN
    }
}

/// Build every slide in the spec (all pages; selection is applied by the
/// caller so it can report the right indices).
pub fn build(spec: &Value, defaults: &Defaults, project_dir: &Path) -> Result<Vec<Scene>, String> {
    let width = spec.get("width").and_then(Value::as_f64).unwrap_or(10.0);
    let height = spec.get("height").and_then(Value::as_f64).unwrap_or(7.5);

    let mut masters: BTreeMap<String, Master> = BTreeMap::new();
    for m in spec
        .get("masters")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
    {
        let name = m.get("name").and_then(Value::as_str).unwrap_or_default();
        masters.insert(name.to_string(), master(m, project_dir)?);
    }

    let mut scenes = Vec::new();
    let mut page = 0usize;
    for section in spec
        .get("sections")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
    {
        for slide in section
            .get("slides")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
        {
            page += 1;
            scenes.push(build_slide(
                slide,
                page,
                width,
                height,
                &masters,
                defaults,
                project_dir,
            )?);
        }
    }
    Ok(scenes)
}

struct Master {
    background: Background,
    margin: Option<Margins>,
    slide_number: Option<Value>,
    chrome: Vec<Value>,
    placeholders: BTreeMap<String, Value>,
}

fn master(m: &Value, dir: &Path) -> Result<Master, String> {
    let mut placeholders = BTreeMap::new();
    let mut chrome = Vec::new();
    for obj in m
        .get("objects")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
    {
        if obj_get(obj, "type").and_then(Value::as_str) == Some("placeholder") {
            if let Some(name) = obj_get(obj, "name").and_then(Value::as_str) {
                placeholders.insert(name.to_string(), obj.clone());
            }
        } else {
            chrome.push(obj.clone());
        }
    }
    Ok(Master {
        background: background(m.get("background"), dir)?,
        margin: m.get("margin").and_then(margins_value),
        slide_number: m.get("slideNumber").filter(|v| !v.is_null()).cloned(),
        chrome,
        placeholders,
    })
}

fn build_slide(
    slide: &Value,
    page: usize,
    width: f64,
    height: f64,
    masters: &BTreeMap<String, Master>,
    defaults: &Defaults,
    dir: &Path,
) -> Result<Scene, String> {
    let master = slide
        .get("master")
        .and_then(Value::as_str)
        .and_then(|name| masters.get(name));
    let slide_bg = background(slide.get("background"), dir)?;
    let (background, mut items) = match master {
        Some(m) => (
            if matches!(slide_bg, Background::None) {
                m.background.clone()
            } else {
                slide_bg
            },
            {
                let mut items = Vec::new();
                for obj in &m.chrome {
                    items.extend(resolve(obj, None, Some(m), defaults, dir)?);
                }
                items
            },
        ),
        None => (slide_bg, Vec::new()),
    };
    let mut approximations = Vec::new();

    if let Some(sn) = master.and_then(|m| m.slide_number.as_ref()) {
        items.push(slide_number_item(sn, page, defaults));
    }

    for obj in slide
        .get("shapes")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
    {
        let ph = obj_get(obj, "placeholder")
            .and_then(Value::as_str)
            .and_then(|name| master.and_then(|m| m.placeholders.get(name)));
        items.extend(resolve(obj, ph, master, defaults, dir)?);
    }

    // A shape that asks for an unknown preset still needs a visible box: draw
    // a dashed rectangle and say so instead of silently dropping the shape.
    for item in &mut items {
        if let ItemKind::Shape { preset, .. } = &item.kind
            && presets::path(
                preset,
                item.rect.w.max(0.01),
                item.rect.h.max(0.01),
                &GeomOpts::default(),
            )
            .is_none()
        {
            approximations.push(format!("shape `{preset}` drawn as a box"));
            item.fill = None;
            item.line = Some(Stroke {
                color: Color::new(0x80, 0x80, 0x80),
                width: 1.0,
                dash: Some("dash".into()),
                head: None,
                tail: None,
            });
        }
    }

    let has_inner_shadow = items.iter().any(|item| {
        item.shadow
            .as_ref()
            .is_some_and(|shadow| shadow.kind == ShadowKind::Inner)
    });
    if has_inner_shadow {
        approximations.push("inner shadows are approximated".to_string());
    }

    Ok(Scene {
        width,
        height,
        background,
        items,
        page,
        file: slide
            .get("file")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        approximations,
    })
}

fn resolve(
    obj: &Value,
    ph: Option<&Value>,
    master: Option<&Master>,
    defaults: &Defaults,
    dir: &Path,
) -> Result<Vec<Item>, String> {
    let kind = obj
        .get("kind")
        .and_then(Value::as_str)
        .or_else(|| obj_get(obj, "type").and_then(Value::as_str))
        .unwrap_or("text");

    let preset = if kind == "shape" {
        obj_get(obj, "type")
            .and_then(Value::as_str)
            .unwrap_or("rect")
    } else if kind == "text" {
        obj_get(obj, "shape").and_then(Value::as_str).unwrap_or("")
    } else {
        ""
    };

    if kind == "chart" || kind == "tbl" || kind == "media" || kind == "placeholder" {
        return Ok(Vec::new());
    }

    let mut rect = Rect {
        x: number(obj_get(obj, "x")).unwrap_or(0.0),
        y: number(obj_get(obj, "y")).unwrap_or(0.0),
        w: number(obj_get(obj, "w")).unwrap_or(0.0),
        h: number(obj_get(obj, "h")).unwrap_or(0.0),
    };
    if let Some(ph) = ph {
        let ph_rect = Rect {
            x: number(obj_get(ph, "x")).unwrap_or(rect.x),
            y: number(obj_get(ph, "y")).unwrap_or(rect.y),
            w: number(obj_get(ph, "w")).unwrap_or(rect.w),
            h: number(obj_get(ph, "h")).unwrap_or(rect.h),
        };
        if rect.w <= 0.0 && rect.h <= 0.0 {
            rect = ph_rect;
        } else {
            if rect.w <= 0.0 {
                rect.w = ph_rect.w;
            }
            if rect.h <= 0.0 {
                rect.h = ph_rect.h;
            }
        }
    }

    let rotate = number(obj_get(obj, "rotate")).unwrap_or(0.0);
    let flip_h = truthy(obj_get(obj, "flipH"));
    let flip_v = truthy(obj_get(obj, "flipV"));
    let fill = fill_value(
        obj_get(obj, "fill").or_else(|| ph.and_then(|p| obj_get(p, "fill"))),
        defaults,
    );
    let line = stroke_value(
        obj_get(obj, "line").or_else(|| ph.and_then(|p| obj_get(p, "line"))),
        defaults,
    );
    let shadow = shadow_value(obj_get(obj, "shadow"), defaults);
    let even_odd = presets::path(
        preset,
        rect.w.max(0.01),
        rect.h.max(0.01),
        &GeomOpts::default(),
    )
    .is_some()
        && presets::uses_evenodd(preset);

    match kind {
        "image" => {
            let data = obj
                .get("data")
                .and_then(Value::as_str)
                .ok_or_else(|| "image shape is missing `data`".to_string())?;
            let sizing = obj_get(obj, "sizing");
            Ok(vec![Item {
                rect,
                rotate,
                flip_h,
                flip_v,
                fill: None,
                line: None,
                shadow,
                even_odd: false,
                kind: ItemKind::Image {
                    data: media::preview_data_uri(data)?,
                    sizing: sizing
                        .and_then(|s| s.get("type"))
                        .and_then(Value::as_str)
                        .unwrap_or("contain")
                        .to_string(),
                    crop: crop_rect(sizing, rect),
                    ellipse: truthy(obj_get(obj, "rounding")),
                    transparency: number(obj_get(obj, "transparency")).unwrap_or(0.0),
                },
            }])
        }
        "shape" => {
            let mut item = Item {
                rect,
                rotate,
                flip_h,
                flip_v,
                fill,
                line,
                shadow,
                even_odd,
                kind: ItemKind::Shape {
                    preset: preset.to_string(),
                    geom: geom_opts(obj),
                },
            };
            if presets::is_open_line(preset) {
                item.fill = None;
            }
            Ok(vec![item])
        }
        _ if !preset.is_empty() && preset != "text" && preset != "placeholder" => {
            // A text box drawn with a preset: the shape paints first, the text
            // sits on top.
            let mut shape = Item {
                rect,
                rotate,
                flip_h,
                flip_v,
                fill,
                line,
                shadow,
                even_odd: presets::uses_evenodd(preset),
                kind: ItemKind::Shape {
                    preset: preset.to_string(),
                    geom: geom_opts(obj),
                },
            };
            if presets::is_open_line(preset) {
                shape.fill = None;
            }
            let text = text_block(obj, ph, master, defaults, dir)?;
            let text = Item {
                rect,
                rotate,
                flip_h,
                flip_v,
                fill: None,
                line: None,
                shadow: None,
                even_odd: false,
                kind: ItemKind::Text(Box::new(text)),
            };
            Ok(vec![shape, text])
        }
        "text" => {
            let text = text_block(obj, ph, master, defaults, dir)?;
            if text.runs.iter().all(|r| r.text.is_empty()) {
                return Ok(Vec::new());
            }
            Ok(vec![Item {
                rect,
                rotate,
                flip_h,
                flip_v,
                fill,
                line,
                shadow,
                even_odd: false,
                kind: ItemKind::Text(Box::new(text)),
            }])
        }
        _ => Ok(Vec::new()),
    }
}

fn text_block(
    obj: &Value,
    ph: Option<&Value>,
    master: Option<&Master>,
    defaults: &Defaults,
    _dir: &Path,
) -> Result<TextBlock, String> {
    let pick = |key: &str| -> Option<&Value> {
        // A slide placeholder's own option map is the flattened style sheet,
        // not an explicit override: pptxgenjs ignores that map and inherits
        // the master placeholder instead. Only use the slide object when no
        // master placeholder matched.
        if ph.is_some() {
            ph.and_then(|p| obj_get(p, key))
        } else {
            obj_get(obj, key)
        }
    };
    let ph_type = ph
        .and_then(|p| option_get(p, "type"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let level_one = defaults.placeholder_style(ph_type, 1);
    let base_owned = match ph {
        Some(p) => placeholder_style(p, level_one, defaults),
        None => defaults.other.clone(),
    };
    let base = &base_owned;

    let margin = pick("margin")
        .and_then(margins_value)
        .or_else(|| master.and_then(|m| m.margin))
        .unwrap_or_else(Margins::default_insets);
    let inset = number(pick("inset"));
    let margin = if let Some(inset) = inset {
        let inches = inset / PT_PER_IN;
        Margins {
            top: inches,
            right: inches,
            bottom: inches,
            left: inches,
        }
    } else {
        margin
    };
    let align = pick("align")
        .and_then(Value::as_str)
        .and_then(|s| Align::parse(Some(s)))
        .or_else(|| base.align.as_deref().and_then(|s| Align::parse(Some(s))))
        .unwrap_or(Align::Left);
    let valign = match pick("valign").and_then(Value::as_str) {
        Some("top") => Valign::Top,
        Some("middle") => Valign::Middle,
        Some("bottom") => Valign::Bottom,
        _ => {
            if ph.is_some() {
                Valign::Top
            } else {
                Valign::Middle
            }
        }
    };
    let wrap = match pick("wrap") {
        Some(v) => truthy(Some(v)),
        None => true,
    };
    let fit = match pick("fit").and_then(Value::as_str) {
        Some("shrink") => Fit::Shrink,
        Some("resize") => Fit::Resize,
        _ => {
            if truthy(pick("shrinkText")) {
                Fit::Shrink
            } else if truthy(pick("autoFit")) {
                Fit::Resize
            } else {
                Fit::None
            }
        }
    };
    let direction = match pick("vert")
        .or_else(|| pick("textDirection"))
        .and_then(Value::as_str)
    {
        Some("vert") => TextDirection::Vert,
        Some("vert270") => TextDirection::Vert270,
        Some("wordArtVert") => TextDirection::WordArtVert,
        _ => TextDirection::Horz,
    };
    let rtl = truthy(pick("rtlMode"));

    let shape_font = pick("fontFace")
        .and_then(Value::as_str)
        .map(|f| resolve_font(Some(f), base, defaults));
    let shape_size = number(pick("fontSize"));
    let shape_color = pick("color").and_then(|v| color_value(v, defaults));
    let shape_bold = truthy(pick("bold"));
    let shape_italic = truthy(pick("italic"));
    let shape_alpha = 1.0 - number(pick("transparency")).unwrap_or(0.0) / 100.0;
    let shape_highlight = pick("highlight").and_then(|v| color_value(v, defaults));

    let mut runs = Vec::new();
    let run_values: Vec<&Value> = obj
        .get("runs")
        .and_then(Value::as_array)
        .map(|a| a.iter().collect())
        .unwrap_or_default();

    for (index, run) in run_values.iter().enumerate() {
        let run_opts = run.get("options");
        let get = |key: &str| -> Option<&Value> {
            run.get(key)
                .or_else(|| run_opts.and_then(|o| o.get(key)))
                .or_else(|| match key {
                    // pptxgenjs copies shape-level text options onto every run,
                    // except `bullet` (first run only) and shape shadows, which
                    // stay on the shape and are not run properties.
                    "bullet" if index == 0 => pick(key),
                    "bullet" | "shadow" => None,
                    _ => pick(key),
                })
        };
        let indent_level = number(get("indentLevel")).unwrap_or(0.0).max(0.0) as usize;
        let level_owned = match ph {
            Some(p) => placeholder_style(
                p,
                defaults.placeholder_style(ph_type, indent_level + 1),
                defaults,
            ),
            None => base.clone(),
        };
        let level = &level_owned;
        let font = resolve_font(get("fontFace").and_then(Value::as_str), level, defaults);
        let size = number(get("fontSize"))
            .or(shape_size)
            .or(level.size_pt)
            .or(base.size_pt)
            .unwrap_or(18.0);
        let hyperlink = get("hyperlink").is_some();
        let color = get("color")
            .and_then(|v| color_value(v, defaults))
            .or(shape_color)
            .unwrap_or(base.color.unwrap_or(Color::BLACK));
        let mut baseline = number(get("baseline")).unwrap_or(0.0);
        if truthy(get("superscript")) {
            baseline = 30.0;
        }
        if truthy(get("subscript")) {
            baseline = -25.0;
        }
        let bullet = bullet_value(get("bullet"));
        let underline = underline_value(get("underline"));
        let strike = truthy(get("strike"));
        let char_spacing = number(get("charSpacing")).unwrap_or(0.0);
        let alpha = match number(get("transparency")) {
            Some(t) => 1.0 - t / 100.0,
            None => shape_alpha,
        };
        let (para_space_before, para_space_after, line_spacing, line_spacing_multiple) =
            if index == 0 || get("paraSpaceBefore").is_some() {
                (
                    number(get("paraSpaceBefore")).unwrap_or(0.0),
                    number(get("paraSpaceAfter")).unwrap_or(0.0),
                    number(get("lineSpacing")),
                    number(get("lineSpacingMultiple")),
                )
            } else {
                (0.0, 0.0, None, None)
            };
        let highlight = match get("highlight").and_then(|v| color_value(v, defaults)) {
            Some(color) => Some(color),
            None => shape_highlight,
        };
        let outline = get("outline").and_then(|v| outline_value(v, defaults));
        let run_glow = glow_value(get("glow"), defaults);
        runs.push(Run {
            text: run
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            font,
            size,
            bold: truthy(get("bold")) || (get("bold").is_none() && (shape_bold || level.bold)),
            italic: truthy(get("italic"))
                || (get("italic").is_none() && (shape_italic || level.italic)),
            underline,
            strike,
            color,
            highlight,
            alpha,
            char_spacing,
            baseline,
            bullet,
            indent_level,
            break_line: truthy(get("breakLine")),
            soft_break_before: truthy(get("softBreakBefore")),
            line_spacing,
            line_spacing_multiple,
            para_space_before,
            para_space_after,
            hyperlink,
            outline,
            glow: run_glow,
        });
    }

    if runs.is_empty()
        && let Some(text) = obj.get("text").and_then(Value::as_str)
        && !text.is_empty()
    {
        runs.push(Run {
            text: text.to_string(),
            font: shape_font
                .clone()
                .unwrap_or_else(|| resolve_font(None, base, defaults)),
            size: shape_size.or(base.size_pt).unwrap_or(18.0),
            bold: shape_bold || base.bold,
            italic: shape_italic || base.italic,
            underline: None,
            strike: false,
            color: shape_color.unwrap_or(base.color.unwrap_or(Color::BLACK)),
            highlight: shape_highlight,
            alpha: shape_alpha,
            char_spacing: 0.0,
            baseline: 0.0,
            bullet: None,
            indent_level: 0,
            break_line: false,
            soft_break_before: false,
            line_spacing: None,
            line_spacing_multiple: None,
            para_space_before: 0.0,
            para_space_after: 0.0,
            hyperlink: false,
            outline: None,
            glow: None,
        });
    }
    if runs.is_empty() {
        runs.push(Run {
            text: String::new(),
            font: shape_font.unwrap_or_else(|| resolve_font(None, base, defaults)),
            size: shape_size.or(base.size_pt).unwrap_or(18.0),
            bold: shape_bold,
            italic: shape_italic,
            underline: None,
            strike: false,
            color: shape_color.unwrap_or(base.color.unwrap_or(Color::BLACK)),
            highlight: shape_highlight,
            alpha: shape_alpha,
            char_spacing: 0.0,
            baseline: 0.0,
            bullet: None,
            indent_level: 0,
            break_line: false,
            soft_break_before: false,
            line_spacing: None,
            line_spacing_multiple: None,
            para_space_before: 0.0,
            para_space_after: 0.0,
            hyperlink: false,
            outline: None,
            glow: None,
        });
    }

    let tab_stops = tab_stops(pick("tabStops"));
    Ok(TextBlock {
        margin,
        align,
        valign,
        wrap,
        fit,
        direction,
        rtl,
        runs,
        line_spacing: number(pick("lineSpacing")),
        line_spacing_multiple: number(pick("lineSpacingMultiple")),
        para_space_before: number(pick("paraSpaceBefore")).unwrap_or(0.0),
        para_space_after: number(pick("paraSpaceAfter")).unwrap_or(0.0),
        tab_stops,
    })
}

/// Apply a master placeholder's option map on top of its level style. The
/// master is the whole text style for a placeholder: slide shape options are
/// ignored by pptxgenjs, while run options still win in `text_block`.
fn placeholder_style(ph: &Value, level: &LevelStyle, defaults: &Defaults) -> LevelStyle {
    let mut style = level.clone();
    if let Some(font) = option_get(ph, "fontFace").and_then(Value::as_str) {
        style.font = Some(resolve_font(Some(font), level, defaults));
    }
    if let Some(size) = number(option_get(ph, "fontSize")) {
        style.size_pt = Some(size);
    }
    if let Some(color) = option_get(ph, "color").and_then(|v| color_value(v, defaults)) {
        style.color = Some(color);
    }
    if let Some(bold) = option_get(ph, "bold") {
        style.bold = truthy(Some(bold));
    }
    if let Some(italic) = option_get(ph, "italic") {
        style.italic = truthy(Some(italic));
    }
    if let Some(align) = option_get(ph, "align").and_then(Value::as_str) {
        style.align = Some(align.to_string());
    }
    style
}

fn resolve_font(explicit: Option<&str>, base: &LevelStyle, defaults: &Defaults) -> String {
    if let Some(name) = explicit
        && !name.trim().is_empty()
    {
        if let Some(role) = defaults.font_role(name) {
            return role.to_string();
        }
        return name.to_string();
    }
    base.font
        .clone()
        .unwrap_or_else(|| defaults.minor_font.clone())
}

fn slide_number_item(sn: &Value, page: usize, defaults: &Defaults) -> Item {
    let rect = Rect {
        x: number(obj_get(sn, "x")).unwrap_or(0.0),
        y: number(obj_get(sn, "y")).unwrap_or(0.0),
        w: number(obj_get(sn, "w")).unwrap_or(1.0),
        h: number(obj_get(sn, "h")).unwrap_or(0.3),
    };
    let size = number(obj_get(sn, "fontSize")).unwrap_or(12.0);
    let color = obj_get(sn, "color")
        .and_then(|v| color_value(v, defaults))
        .unwrap_or(Color::BLACK);
    let font = obj_get(sn, "fontFace")
        .and_then(Value::as_str)
        .map(|f| resolve_font(Some(f), &defaults.other, defaults))
        .unwrap_or_else(|| defaults.minor_font.clone());
    let align = match obj_get(sn, "align").and_then(Value::as_str) {
        Some("left") => Align::Left,
        Some("right") => Align::Right,
        _ => Align::Center,
    };
    let text = TextBlock {
        margin: Margins::default_insets(),
        align,
        valign: Valign::Middle,
        wrap: false,
        fit: Fit::None,
        direction: TextDirection::Horz,
        rtl: false,
        runs: vec![Run {
            text: page.to_string(),
            font,
            size,
            bold: truthy(obj_get(sn, "bold")),
            italic: false,
            underline: None,
            strike: false,
            color,
            highlight: None,
            alpha: 1.0,
            char_spacing: 0.0,
            baseline: 0.0,
            bullet: None,
            indent_level: 0,
            break_line: false,
            soft_break_before: false,
            line_spacing: None,
            line_spacing_multiple: None,
            para_space_before: 0.0,
            para_space_after: 0.0,
            hyperlink: false,
            outline: None,
            glow: None,
        }],
        line_spacing: None,
        line_spacing_multiple: None,
        para_space_before: 0.0,
        para_space_after: 0.0,
        tab_stops: Vec::new(),
    };
    Item {
        rect,
        rotate: 0.0,
        flip_h: false,
        flip_v: false,
        fill: None,
        line: None,
        shadow: None,
        even_odd: false,
        kind: ItemKind::Text(Box::new(text)),
    }
}

// --- spec helpers ---------------------------------------------------------

/// Read a key from a slide shape (flat) or a master object (inside `options`).
fn obj_get<'a>(obj: &'a Value, key: &str) -> Option<&'a Value> {
    obj.get(key)
        .or_else(|| obj.get("options").and_then(|o| o.get(key)))
}

/// Read a master object's option map before its structural keys. A master
/// placeholder has `"type": "placeholder"` at the top level but the real
/// placeholder type lives in `options.type`.
fn option_get<'a>(obj: &'a Value, key: &str) -> Option<&'a Value> {
    obj.get("options")
        .and_then(|o| o.get(key))
        .or_else(|| obj.get(key))
}

fn number(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

fn truthy(v: Option<&Value>) -> bool {
    match v {
        Some(Value::Bool(value)) => *value,
        Some(Value::String(text)) => matches!(text.as_str(), "true" | "1" | "yes"),
        Some(Value::Number(number)) => number.as_f64().is_some_and(|n| n != 0.0),
        _ => false,
    }
}

fn margins_value(v: &Value) -> Option<Margins> {
    let mut vals = [0.0f64; 4]; // top, right, bottom, left
    if let Some(list) = v.as_array() {
        for (slot, item) in vals.iter_mut().zip(list.iter()) {
            *slot = number(Some(item)).unwrap_or(0.0);
        }
    } else {
        let n = number(Some(v))?;
        vals = [n; 4];
    }
    // Numeric margins are points in pptxgenjs; unit strings were converted to
    // fractional inches by the spec builder.
    let all_int = vals.iter().all(|n| n.fract().abs() < 1e-9);
    if all_int && vals.iter().any(|n| *n > 0.0) {
        for n in &mut vals {
            *n /= PT_PER_IN;
        }
    }
    let [top, right, bottom, left] = vals;
    Some(Margins {
        top,
        right,
        bottom,
        left,
    })
}

fn tab_stops(v: Option<&Value>) -> Vec<f64> {
    let Some(list) = v.and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|stop| number(stop.get("position")))
        .collect()
}

fn crop_rect(sizing: Option<&Value>, rect: Rect) -> Option<Rect> {
    let sizing = sizing?;
    let kind = sizing.get("type").and_then(Value::as_str)?;
    if kind != "crop" {
        return None;
    }
    Some(Rect {
        x: number(sizing.get("x")).unwrap_or(0.0),
        y: number(sizing.get("y")).unwrap_or(0.0),
        w: number(sizing.get("w")).unwrap_or(rect.w),
        h: number(sizing.get("h")).unwrap_or(rect.h),
    })
}

fn geom_opts(obj: &Value) -> GeomOpts {
    GeomOpts {
        rect_radius: number(obj_get(obj, "rectRadius")),
        arc_thickness_ratio: number(obj_get(obj, "arcThicknessRatio")),
        angle_range: obj_get(obj, "angleRange")
            .and_then(Value::as_array)
            .filter(|items| items.len() == 2)
            .and_then(|items| Some((number(Some(&items[0]))?, number(Some(&items[1]))?))),
        points: points_value(obj_get(obj, "points")),
    }
}

fn outline_value(v: &Value, defaults: &Defaults) -> Option<TextOutline> {
    let map = v.as_object()?;
    Some(TextOutline {
        color: map
            .get("color")
            .and_then(|c| color_value(c, defaults))
            .unwrap_or(Color::BLACK),
        size: number(map.get("size")).unwrap_or(1.0),
    })
}

fn points_value(v: Option<&Value>) -> Option<Vec<Point>> {
    let list = v?.as_array()?;
    let mut out = Vec::with_capacity(list.len());
    for p in list {
        let curve = p.get("curve").and_then(|c| {
            let kind = c.get("type").and_then(Value::as_str)?;
            Some(match kind {
                "arc" => Curve::Arc {
                    hr: number(c.get("hR")).unwrap_or(0.0),
                    wr: number(c.get("wR")).unwrap_or(0.0),
                    st_ang: number(c.get("stAng")).unwrap_or(0.0),
                    sw_ang: number(c.get("swAng")).unwrap_or(0.0),
                },
                "cubic" => Curve::Cubic {
                    x1: number(c.get("x1")).unwrap_or(0.0),
                    y1: number(c.get("y1")).unwrap_or(0.0),
                    x2: number(c.get("x2")).unwrap_or(0.0),
                    y2: number(c.get("y2")).unwrap_or(0.0),
                },
                "quadratic" => Curve::Quadratic {
                    x1: number(c.get("x1")).unwrap_or(0.0),
                    y1: number(c.get("y1")).unwrap_or(0.0),
                },
                _ => return None,
            })
        });
        out.push(Point {
            x: number(p.get("x")).unwrap_or(0.0),
            y: number(p.get("y")).unwrap_or(0.0),
            move_to: truthy(p.get("moveTo")),
            close: truthy(p.get("close")),
            curve,
        });
    }
    Some(out)
}

fn color_value(v: &Value, defaults: &Defaults) -> Option<Color> {
    match v {
        Value::String(s) => resolve_color_name(s, defaults),
        Value::Object(map) => {
            if map.get("type").and_then(Value::as_str) == Some("none") {
                return None;
            }
            let color = map.get("color")?;
            let mut c = match color {
                Value::String(s) => resolve_color_name(s, defaults)?,
                Value::Object(inner) => {
                    let name = inner.get("color").and_then(Value::as_str)?;
                    resolve_color_name(name, defaults)?
                }
                _ => return None,
            };
            if let Some(t) = number(map.get("transparency")) {
                c = c.with_alpha(1.0 - t / 100.0);
            } else if let Some(a) = number(map.get("alpha")) {
                c = c.with_alpha(if a > 1.0 { a / 100.0 } else { a });
            }
            Some(c)
        }
        _ => None,
    }
}

fn resolve_color_name(name: &str, defaults: &Defaults) -> Option<Color> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    match name.to_ascii_lowercase().as_str() {
        "transparent" | "none" => None,
        "black" => Some(Color::BLACK),
        "white" => Some(Color::WHITE),
        _ => Color::hex(name).or_else(|| defaults.theme_color(name)),
    }
}

fn fill_value(v: Option<&Value>, defaults: &Defaults) -> Option<Color> {
    let v = v?;
    match v {
        Value::String(s) => resolve_color_name(s, defaults),
        Value::Object(map) => {
            if map.get("type").and_then(Value::as_str) == Some("none") {
                return None;
            }
            color_value(v, defaults)
        }
        _ => None,
    }
}

fn stroke_value(v: Option<&Value>, defaults: &Defaults) -> Option<Stroke> {
    let v = v?;
    match v {
        Value::String(s) => Some(Stroke {
            color: resolve_color_name(s, defaults).unwrap_or(Color::BLACK),
            width: 1.0,
            dash: None,
            head: None,
            tail: None,
        }),
        Value::Object(map) => {
            if map.get("type").and_then(Value::as_str) == Some("none") {
                return None;
            }
            let color = map
                .get("color")
                .and_then(|c| color_value(c, defaults))
                .unwrap_or(Color::BLACK);
            let width = number(map.get("width")).unwrap_or(1.0);
            let dash = map
                .get("dashType")
                .and_then(Value::as_str)
                .filter(|d| *d != "solid")
                .map(String::from);
            let head = map
                .get("beginArrowType")
                .and_then(Value::as_str)
                .filter(|d| *d != "none")
                .map(String::from);
            let tail = map
                .get("endArrowType")
                .and_then(Value::as_str)
                .filter(|d| *d != "none")
                .map(String::from);
            Some(Stroke {
                color,
                width,
                dash,
                head,
                tail,
            })
        }
        _ => None,
    }
}

fn shadow_value(v: Option<&Value>, defaults: &Defaults) -> Option<Shadow> {
    let map = v?.as_object()?;
    let kind = match map.get("type").and_then(Value::as_str) {
        Some("inner") => ShadowKind::Inner,
        Some("none") => return None,
        _ => ShadowKind::Outer,
    };
    Some(Shadow {
        kind,
        color: map
            .get("color")
            .and_then(|c| color_value(c, defaults))
            .unwrap_or(Color::BLACK),
        opacity: number(map.get("opacity")).unwrap_or(0.75),
        blur: number(map.get("blur")).unwrap_or(8.0),
        angle: number(map.get("angle")).unwrap_or(270.0),
        distance: number(map.get("offset"))
            .or_else(|| number(map.get("distance")))
            .unwrap_or(4.0),
        rotate_with_shape: truthy(map.get("rotateWithShape")),
    })
}

fn glow_value(v: Option<&Value>, defaults: &Defaults) -> Option<Glow> {
    let map = v?.as_object()?;
    if ["color", "size", "opacity"]
        .iter()
        .all(|k| map.get(*k).is_none())
    {
        return None;
    }
    Some(Glow {
        color: map
            .get("color")
            .and_then(|c| color_value(c, defaults))
            .unwrap_or(Color::new(0xFF, 0xD9, 0x66)),
        opacity: number(map.get("opacity")).unwrap_or(0.4),
        size: number(map.get("size")).unwrap_or(10.0),
    })
}

fn underline_value(v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::Bool(true)) => Some("sng".into()),
        Some(Value::Bool(false)) | None => None,
        Some(Value::String(s)) if s != "none" => Some(s.clone()),
        Some(Value::Object(map)) => map
            .get("style")
            .and_then(Value::as_str)
            .filter(|s| *s != "none")
            .map(String::from),
        _ => None,
    }
}

fn bullet_value(v: Option<&Value>) -> Option<Bullet> {
    let v = v?;
    match v {
        Value::Bool(true) => Some(Bullet::Char {
            text: "\u{2022}".into(),
            indent_pt: 27.0,
        }),
        Value::String(s) if !s.is_empty() => Some(Bullet::Char {
            text: s.clone(),
            indent_pt: 27.0,
        }),
        Value::Object(map) => {
            let indent_pt = number(map.get("indent")).unwrap_or(27.0);
            if map.get("type").and_then(Value::as_str) == Some("number") {
                return Some(Bullet::Number {
                    style: map
                        .get("style")
                        .or_else(|| map.get("numberType"))
                        .and_then(Value::as_str)
                        .unwrap_or("arabicPeriod")
                        .to_string(),
                    start: number(map.get("numberStartAt"))
                        .or_else(|| number(map.get("startAt")))
                        .unwrap_or(1.0)
                        .max(1.0) as usize,
                    indent_pt,
                });
            }
            if let Some(code) = map
                .get("characterCode")
                .or_else(|| map.get("code"))
                .and_then(Value::as_str)
            {
                let ch = u32::from_str_radix(code.trim_start_matches("0x"), 16)
                    .ok()
                    .and_then(char::from_u32)
                    .unwrap_or('\u{2022}');
                return Some(Bullet::Char {
                    text: ch.to_string(),
                    indent_pt,
                });
            }
            if let Some(ch) = map.get("character").and_then(Value::as_str) {
                return Some(Bullet::Char {
                    text: ch.to_string(),
                    indent_pt,
                });
            }
            Some(Bullet::Char {
                text: "\u{2022}".into(),
                indent_pt,
            })
        }
        _ => None,
    }
}

fn background(v: Option<&Value>, dir: &Path) -> Result<Background, String> {
    let Some(v) = v.filter(|v| !v.is_null()) else {
        return Ok(Background::None);
    };
    if let Some(color) = v.as_str() {
        return Ok(match Color::hex(color) {
            Some(c) => Background::Color(c),
            None => Background::None,
        });
    }
    let map = v
        .as_object()
        .ok_or("background must be a color or object")?;
    let transparency = number(map.get("transparency")).unwrap_or(0.0);
    if let Some(data) = map.get("data").and_then(Value::as_str) {
        return Ok(Background::Image {
            data: media::preview_data_uri(data)?,
            transparency,
        });
    }
    if let Some(path) = map
        .get("path")
        .or_else(|| map.get("src"))
        .and_then(Value::as_str)
    {
        let bytes = std::fs::read(dir.join(path))
            .map_err(|e| format!("cannot read background `{path}`: {e}"))?;
        let mime = media::mime_for(path).map_err(|e| format!("background `{path}`: {e}"))?;
        let data = format!("data:{mime};base64,{}", base64_encode(&bytes));
        return Ok(Background::Image {
            data: media::preview_data_uri(&data)?,
            transparency,
        });
    }
    if let Some(color) = map.get("color").or_else(|| map.get("fill")).and_then(|c| {
        if let Some(s) = c.as_str() {
            Color::hex(s)
        } else {
            c.get("color").and_then(Value::as_str).and_then(Color::hex)
        }
    }) {
        return Ok(Background::Color(
            color.with_alpha(1.0 - transparency / 100.0),
        ));
    }
    Ok(Background::None)
}

fn base64_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn margins_are_points_when_whole_numbers() {
        let m = margins_value(&serde_json::json!([5, 10, 5, 10])).unwrap();
        assert!((m.top - 5.0 / 72.0).abs() < 1e-9);
        assert!((m.left - 10.0 / 72.0).abs() < 1e-9);
        let m = margins_value(&serde_json::json!(0.1)).unwrap();
        assert!((m.top - 0.1).abs() < 1e-9);
    }

    #[test]
    fn bullets_parse_character_and_number() {
        assert!(matches!(
            bullet_value(Some(&serde_json::json!(true))),
            Some(Bullet::Char { ref text, .. }) if text == "\u{2022}"
        ));
        assert!(matches!(
            bullet_value(Some(&serde_json::json!({"characterCode": "25AA"}))),
            Some(Bullet::Char { ref text, .. }) if text == "\u{25AA}"
        ));
        assert!(matches!(
            bullet_value(Some(
                &serde_json::json!({"type":"number","style":"alphaLcPeriod","numberStartAt":3})
            )),
            Some(Bullet::Number { start: 3, .. })
        ));
        assert!(bullet_value(None).is_none());
    }

    #[test]
    fn colors_resolve_hex_and_theme() {
        let palette = BTreeMap::from([("accent1".to_string(), Color::new(1, 2, 3))]);
        let d = Defaults {
            palette,
            major_font: "A".into(),
            minor_font: "B".into(),
            title: LevelStyle::default(),
            body: vec![LevelStyle::default()],
            other: LevelStyle::default(),
        };
        assert_eq!(resolve_color_name("accent1", &d), Some(Color::new(1, 2, 3)));
        assert_eq!(
            resolve_color_name("#00FF00", &d),
            Some(Color::new(0, 255, 0))
        );
        assert_eq!(resolve_color_name("transparent", &d), None);
    }
}
