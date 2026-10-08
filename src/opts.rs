//! Validate the free-form pptxgenjs option maps gwen passes through, so typos
//! like `fount_size` or a stray `transperency` inside `fill` fail at build time
//! instead of being silently ignored by pptxgenjs.
//!
//! The whitelists are in snake_case (the TOML form) and mirror the vendored
//! pptxgenjs 4.0.1 `index.d.ts`; keep them in sync with
//! `src/js/pptxgen.bundle.js` when that bundle is upgraded.

use miette::{Result, miette};

use toml_edit::Value;

/// The pptxgenjs `ShapeType` preset ids (from the vendored `index.d.ts`
/// `SHAPE_NAME`), used to validate shape `type` and `[styles.<type>]` keys.
pub const SHAPE_PRESETS: &[&str] = &[
    "accentBorderCallout1",
    "accentBorderCallout2",
    "accentBorderCallout3",
    "accentCallout1",
    "accentCallout2",
    "accentCallout3",
    "actionButtonBackPrevious",
    "actionButtonBeginning",
    "actionButtonBlank",
    "actionButtonDocument",
    "actionButtonEnd",
    "actionButtonForwardNext",
    "actionButtonHelp",
    "actionButtonHome",
    "actionButtonInformation",
    "actionButtonMovie",
    "actionButtonReturn",
    "actionButtonSound",
    "arc",
    "bentArrow",
    "bentUpArrow",
    "bevel",
    "blockArc",
    "borderCallout1",
    "borderCallout2",
    "borderCallout3",
    "bracePair",
    "bracketPair",
    "callout1",
    "callout2",
    "callout3",
    "can",
    "chartPlus",
    "chartStar",
    "chartX",
    "chevron",
    "chord",
    "circularArrow",
    "cloud",
    "cloudCallout",
    "corner",
    "cornerTabs",
    "cube",
    "curvedDownArrow",
    "curvedLeftArrow",
    "curvedRightArrow",
    "curvedUpArrow",
    "decagon",
    "diagStripe",
    "diamond",
    "dodecagon",
    "donut",
    "doubleWave",
    "downArrow",
    "downArrowCallout",
    "ellipse",
    "ellipseRibbon",
    "ellipseRibbon2",
    "flowChartAlternateProcess",
    "flowChartCollate",
    "flowChartConnector",
    "flowChartDecision",
    "flowChartDelay",
    "flowChartDisplay",
    "flowChartDocument",
    "flowChartExtract",
    "flowChartInputOutput",
    "flowChartInternalStorage",
    "flowChartMagneticDisk",
    "flowChartMagneticDrum",
    "flowChartMagneticTape",
    "flowChartManualInput",
    "flowChartManualOperation",
    "flowChartMerge",
    "flowChartMultidocument",
    "flowChartOfflineStorage",
    "flowChartOffpageConnector",
    "flowChartOnlineStorage",
    "flowChartOr",
    "flowChartPredefinedProcess",
    "flowChartPreparation",
    "flowChartProcess",
    "flowChartPunchedCard",
    "flowChartPunchedTape",
    "flowChartSort",
    "flowChartSummingJunction",
    "flowChartTerminator",
    "folderCorner",
    "frame",
    "funnel",
    "gear6",
    "gear9",
    "halfFrame",
    "heart",
    "heptagon",
    "hexagon",
    "homePlate",
    "horizontalScroll",
    "irregularSeal1",
    "irregularSeal2",
    "leftArrow",
    "leftArrowCallout",
    "leftBrace",
    "leftBracket",
    "leftCircularArrow",
    "leftRightArrow",
    "leftRightArrowCallout",
    "leftRightCircularArrow",
    "leftRightRibbon",
    "leftRightUpArrow",
    "leftUpArrow",
    "lightningBolt",
    "line",
    "lineInv",
    "mathDivide",
    "mathEqual",
    "mathMinus",
    "mathMultiply",
    "mathNotEqual",
    "mathPlus",
    "moon",
    "noSmoking",
    "nonIsoscelesTrapezoid",
    "notchedRightArrow",
    "octagon",
    "parallelogram",
    "pentagon",
    "pie",
    "pieWedge",
    "plaque",
    "plaqueTabs",
    "plus",
    "quadArrow",
    "quadArrowCallout",
    "rect",
    "ribbon",
    "ribbon2",
    "rightArrow",
    "rightArrowCallout",
    "rightBrace",
    "rightBracket",
    "round1Rect",
    "round2DiagRect",
    "round2SameRect",
    "roundRect",
    "rtTriangle",
    "smileyFace",
    "snip1Rect",
    "snip2DiagRect",
    "snip2SameRect",
    "snipRoundRect",
    "squareTabs",
    "star10",
    "star12",
    "star16",
    "star24",
    "star32",
    "star4",
    "star5",
    "star6",
    "star7",
    "star8",
    "stripedRightArrow",
    "sun",
    "swooshArrow",
    "teardrop",
    "trapezoid",
    "triangle",
    "upArrow",
    "upArrowCallout",
    "upDownArrow",
    "upDownArrowCallout",
    "uturnArrow",
    "verticalScroll",
    "wave",
    "wedgeEllipseCallout",
    "wedgeRectCallout",
    "wedgeRoundRectCallout",
];

/// `snake_case -> camelCase` (used to canonicalise shape preset names, so
/// `round_rect` and `roundRect` both resolve).
pub fn snake_to_camel(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut upper_next = false;
    for c in s.chars() {
        if c == '_' {
            upper_next = true;
        } else if upper_next {
            out.push(c.to_ascii_uppercase());
            upper_next = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// `camelCase -> snake_case` (the DSL form of a preset id or option key).
pub fn camel_to_snake(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        if c.is_ascii_uppercase() {
            if !out.is_empty() {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// Resolve a shape `type` to its canonical pptxgenjs preset id, accepting
/// snake_case (`round_rect`) or the camelCase preset (`roundRect`).
pub fn canonical_preset(ty: &str) -> Option<&'static str> {
    let camel = snake_to_camel(ty);
    SHAPE_PRESETS.iter().find(|p| **p == camel).copied()
}

/// Whether a shape `type` is a valid slide shape type (`text`, `image`, or a
/// shape preset). Chart/table/media are handled separately as reserved.
pub fn is_shape_type(ty: &str) -> bool {
    matches!(ty, "text" | "image") || canonical_preset(ty).is_some()
}

/// Which option context a `[styles.<type>]` bucket feeds. `shape` is the
/// fallback for every shape (including text/image), so it uses the union.
pub fn ctx_for_style_type(ty: &str) -> Option<Ctx> {
    match ty {
        "shape" => Some(Ctx::Style),
        "text" => Some(Ctx::Text),
        "image" => Some(Ctx::Image),
        "placeholder" => Some(Ctx::Placeholder),
        p if SHAPE_PRESETS.contains(&p) => Some(Ctx::TextShape),
        _ => None,
    }
}

/// Which pptxgenjs option object a TOML option map feeds.
#[derive(Clone, Copy)]
pub enum Ctx {
    /// Text shapes, master text/placeholder options, paragraph options,
    /// `slide_number`.
    Text,
    /// Master placeholder options: text options plus `name`/`ph_type`.
    Placeholder,
    /// Shape presets (rect, roundRect, line, ...): shape options plus text
    /// options, because any preset may carry text.
    TextShape,
    /// Images.
    Image,
    /// Slide/master backgrounds.
    Background,
    /// Named styles: the union of text, shape and image options.
    Style,
}

const POSITION: &[&str] = &["x", "y", "w", "h"];

pub const TEXT_KEYS: &[&str] = &[
    "align",
    "bold",
    "break_line",
    "bullet",
    "color",
    "font_face",
    "font_size",
    "highlight",
    "italic",
    "lang",
    "soft_break_before",
    "tab_stops",
    "text_direction",
    "transparency",
    "underline",
    "valign",
    "baseline",
    "char_spacing",
    "fit",
    "fill",
    "flip_h",
    "flip_v",
    "glow",
    "hyperlink",
    "indent_level",
    "is_text_box",
    "line",
    "line_spacing",
    "line_spacing_multiple",
    "margin",
    "outline",
    "para_space_after",
    "para_space_before",
    "placeholder",
    "rect_radius",
    "rotate",
    "rtl_mode",
    "shadow",
    "shape",
    "strike",
    "subscript",
    "superscript",
    "vert",
    "wrap",
    "auto_fit",
    "shrink_text",
    "inset",
    "object_name",
    "path",
    "data",
    "line_dash",
    "line_head",
    "line_size",
    "line_tail",
    "ordered_markers",
    "unordered_markers",
    "styles",
    "sizing",
];

/// Shape presets can carry text (rect/ellipse/... with a `text` value), so
/// their option maps accept both shape and text keys.
pub const TEXTSHAPE_KEYS: &[&str] = &[
    "align",
    "bold",
    "break_line",
    "bullet",
    "color",
    "font_face",
    "font_size",
    "highlight",
    "italic",
    "lang",
    "soft_break_before",
    "tab_stops",
    "text_direction",
    "transparency",
    "underline",
    "valign",
    "baseline",
    "char_spacing",
    "fit",
    "fill",
    "flip_h",
    "flip_v",
    "glow",
    "hyperlink",
    "indent_level",
    "is_text_box",
    "line",
    "line_spacing",
    "line_spacing_multiple",
    "margin",
    "outline",
    "para_space_after",
    "para_space_before",
    "placeholder",
    "rect_radius",
    "rotate",
    "rtl_mode",
    "shadow",
    "shape",
    "strike",
    "subscript",
    "superscript",
    "vert",
    "wrap",
    "auto_fit",
    "shrink_text",
    "inset",
    "object_name",
    "path",
    "data",
    "line_dash",
    "line_head",
    "line_size",
    "line_tail",
    "ordered_markers",
    "unordered_markers",
    "styles",
    "angle_range",
    "arc_thickness_ratio",
    "points",
    "shape_name",
    "sizing",
];

pub const IMAGE_KEYS: &[&str] = &[
    "alt_text",
    "flip_h",
    "flip_v",
    "hyperlink",
    "placeholder",
    "rotate",
    "rounding",
    "shadow",
    "sizing",
    "transparency",
    "object_name",
    "path",
    "data",
    "fill",
    "line",
    "bullet",
    "underline",
    "outline",
    "glow",
];

pub const BACKGROUND_KEYS: &[&str] = &[
    "color",
    "transparency",
    "type",
    "alpha",
    "path",
    "data",
    "fill",
    "src",
];

const STYLE_KEYS: &[&str] = &[
    "align",
    "bold",
    "break_line",
    "bullet",
    "color",
    "font_face",
    "font_size",
    "highlight",
    "italic",
    "lang",
    "soft_break_before",
    "tab_stops",
    "text_direction",
    "transparency",
    "underline",
    "valign",
    "baseline",
    "char_spacing",
    "fit",
    "fill",
    "flip_h",
    "flip_v",
    "glow",
    "hyperlink",
    "indent_level",
    "is_text_box",
    "line",
    "line_spacing",
    "line_spacing_multiple",
    "margin",
    "outline",
    "para_space_after",
    "para_space_before",
    "placeholder",
    "rect_radius",
    "rotate",
    "rtl_mode",
    "shadow",
    "shape",
    "strike",
    "subscript",
    "superscript",
    "vert",
    "wrap",
    "auto_fit",
    "shrink_text",
    "inset",
    "object_name",
    "path",
    "data",
    "line_dash",
    "line_head",
    "line_size",
    "line_tail",
    "ordered_markers",
    "unordered_markers",
    "styles",
    "angle_range",
    "arc_thickness_ratio",
    "points",
    "shape_name",
    "alt_text",
    "rounding",
    "sizing",
];

/// Value kinds for pptxgenjs option values, derived from the vendored `index.d.ts`.
#[derive(Debug, Clone, Copy)]
pub enum Kind {
    /// No value check.
    Any,
    Bool,
    /// Number within an inclusive `[min,max]` range.
    Num(f64, f64),
    Str,
    /// 6-hex color or a pptxgenjs theme color.
    Color,
    Enum(&'static [&'static str]),
    /// A shape preset id (snake or camel).
    ShapePreset,
    /// EMU integer, a unit string (`"1cm"`) or a percentage (`"50%"`).
    Coord,
    /// A number or a 4-number `[top, right, bottom, left]`.
    Margin,
    /// Exactly two numbers within `[min,max]`.
    Num2(f64, f64),
    Tables(&'static [(&'static str, Kind)]),
    /// A table or a string shorthand (`fill`/`line`).
    ObjOrStr(&'static [(&'static str, Kind)]),
    /// `true`/`false` or a table (`bullet`/`underline`).
    BoolOrObj(&'static [(&'static str, Kind)]),
    /// `true`/`false`, a string shorthand, or a table (`underline`).
    BoolOrStrOrObj(&'static [(&'static str, Kind)]),
    TabStops,
    Points,
    ArrayOfTables,
}

const fn num() -> Kind {
    Kind::Num(f64::NEG_INFINITY, f64::INFINITY)
}
const fn rng(lo: f64, hi: f64) -> Kind {
    Kind::Num(lo, hi)
}

const HALIGN: &[&str] = &["left", "center", "right", "justify"];
const VALIGN: &[&str] = &["top", "middle", "bottom"];
const FIT: &[&str] = &["none", "shrink", "resize"];
const TEXTDIR: &[&str] = &["horz", "vert", "vert270", "wordArtVert"];
const SHADOW_TYPE: &[&str] = &["outer", "inner", "none"];
const FILL_TYPE: &[&str] = &["none", "solid"];
const LINE_DASH: &[&str] = &[
    "solid",
    "dash",
    "dashDot",
    "lgDash",
    "lgDashDot",
    "lgDashDotDot",
    "sysDash",
    "sysDot",
];
const ARROW_TYPE: &[&str] = &["none", "arrow", "diamond", "oval", "stealth", "triangle"];
const SIZING_TYPE: &[&str] = &["contain", "cover", "crop"];
const BULLET_TYPE: &[&str] = &["bullet", "number"];
const TAB_ALIGN: &[&str] = &["l", "r", "ctr", "dec"];
const THEME_COLORS: &[&str] = &[
    "tx1", "tx2", "bg1", "bg2", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6",
];
const UNDERLINE_STYLE: &[&str] = &[
    "dash",
    "dashHeavy",
    "dashLong",
    "dashLongHeavy",
    "dbl",
    "dotDash",
    "dotDashHeave",
    "dotDotDash",
    "dotDotDashHeavy",
    "dotted",
    "dottedHeavy",
    "heavy",
    "none",
    "sng",
    "wavy",
    "wavyDbl",
    "wavyHeavy",
];
const BULLET_NUMBER_TYPE: &[&str] = &[
    "alphaLcParenBoth",
    "alphaLcParenR",
    "alphaLcPeriod",
    "alphaUcParenBoth",
    "alphaUcParenR",
    "alphaUcPeriod",
    "arabicParenBoth",
    "arabicParenR",
    "arabicPeriod",
    "arabicPlain",
    "romanLcParenBoth",
    "romanLcParenR",
    "romanLcPeriod",
    "romanUcParenBoth",
    "romanUcParenR",
    "romanUcPeriod",
];
const CURVE_TYPE: &[&str] = &["arc", "cubic", "quadratic"];

pub fn fill_schema() -> &'static [(&'static str, Kind)] {
    static V: std::sync::OnceLock<Vec<(&'static str, Kind)>> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        vec![
            ("color", Kind::Color),
            ("transparency", rng(0.0, 100.0)),
            ("type", Kind::Enum(FILL_TYPE)),
            ("alpha", rng(0.0, 100.0)),
        ]
    })
    .as_slice()
}

pub fn line_schema() -> &'static [(&'static str, Kind)] {
    static V: std::sync::OnceLock<Vec<(&'static str, Kind)>> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        vec![
            ("color", Kind::Color),
            ("transparency", rng(0.0, 100.0)),
            ("type", Kind::Enum(FILL_TYPE)),
            ("alpha", rng(0.0, 100.0)),
            ("width", num()),
            ("dash_type", Kind::Enum(LINE_DASH)),
            ("begin_arrow_type", Kind::Enum(ARROW_TYPE)),
            ("end_arrow_type", Kind::Enum(ARROW_TYPE)),
            ("line_dash", Kind::Enum(LINE_DASH)),
            ("line_head", Kind::Enum(ARROW_TYPE)),
            ("line_tail", Kind::Enum(ARROW_TYPE)),
            ("pt", num()),
            ("size", num()),
        ]
    })
    .as_slice()
}

pub fn shadow_schema() -> &'static [(&'static str, Kind)] {
    static V: std::sync::OnceLock<Vec<(&'static str, Kind)>> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        vec![
            ("type", Kind::Enum(SHADOW_TYPE)),
            ("opacity", rng(0.0, 1.0)),
            ("blur", rng(0.0, 100.0)),
            ("angle", rng(0.0, 359.0)),
            ("offset", rng(0.0, 200.0)),
            ("color", Kind::Color),
            ("rotate_with_shape", Kind::Bool),
        ]
    })
    .as_slice()
}

pub fn bullet_schema() -> &'static [(&'static str, Kind)] {
    static V: std::sync::OnceLock<Vec<(&'static str, Kind)>> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        vec![
            ("type", Kind::Enum(BULLET_TYPE)),
            ("character_code", Kind::Str),
            ("indent", num()),
            ("number_type", Kind::Enum(BULLET_NUMBER_TYPE)),
            ("number_start_at", num()),
            ("code", Kind::Str),
            ("margin_pt", num()),
            ("start_at", num()),
            ("style", Kind::Str),
        ]
    })
    .as_slice()
}

pub fn sizing_schema() -> &'static [(&'static str, Kind)] {
    static V: std::sync::OnceLock<Vec<(&'static str, Kind)>> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        vec![
            ("type", Kind::Enum(SIZING_TYPE)),
            ("w", Kind::Coord),
            ("h", Kind::Coord),
            ("x", Kind::Coord),
            ("y", Kind::Coord),
        ]
    })
    .as_slice()
}

pub fn hyperlink_schema() -> &'static [(&'static str, Kind)] {
    static V: std::sync::OnceLock<Vec<(&'static str, Kind)>> = std::sync::OnceLock::new();
    V.get_or_init(|| vec![("slide", num()), ("url", Kind::Str), ("tooltip", Kind::Str)])
        .as_slice()
}

pub fn underline_schema() -> &'static [(&'static str, Kind)] {
    static V: std::sync::OnceLock<Vec<(&'static str, Kind)>> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        vec![
            ("style", Kind::Enum(UNDERLINE_STYLE)),
            ("color", Kind::Color),
        ]
    })
    .as_slice()
}

pub fn outline_schema() -> &'static [(&'static str, Kind)] {
    static V: std::sync::OnceLock<Vec<(&'static str, Kind)>> = std::sync::OnceLock::new();
    V.get_or_init(|| vec![("color", Kind::Color), ("size", num())])
        .as_slice()
}

pub fn glow_schema() -> &'static [(&'static str, Kind)] {
    static V: std::sync::OnceLock<Vec<(&'static str, Kind)>> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        vec![
            ("color", Kind::Color),
            ("opacity", rng(0.0, 1.0)),
            ("size", num()),
        ]
    })
    .as_slice()
}

/// The value kind for an option key, or `None` to leave the value unchecked.
fn kind_for(ctx: Ctx, key: &str) -> Option<Kind> {
    // Background-specific keys.
    if matches!(ctx, Ctx::Background) {
        return match key {
            "color" | "fill" => Some(Kind::Color),
            "transparency" | "alpha" => Some(rng(0.0, 100.0)),
            "type" => Some(Kind::Enum(FILL_TYPE)),
            "path" | "data" | "src" => Some(Kind::Str),
            _ => Some(Kind::Any),
        };
    }
    let k = match key {
        "bold" | "italic" | "strike" | "subscript" | "superscript" | "wrap" | "break_line"
        | "soft_break_before" | "flip_h" | "flip_v" | "rtl_mode" | "auto_fit" | "shrink_text"
        | "is_text_box" => Kind::Bool,
        "align" => Kind::Enum(HALIGN),
        "valign" => Kind::Enum(VALIGN),
        "fit" => Kind::Enum(FIT),
        "text_direction" | "vert" => Kind::Enum(TEXTDIR),
        "shape" => Kind::ShapePreset,
        "rect_radius" | "arc_thickness_ratio" => rng(0.0, 1.0),
        "rotate" => rng(-360.0, 360.0),
        "transparency" => rng(0.0, 100.0),
        "angle_range" => Kind::Num2(0.0, 359.0),
        "font_size"
        | "char_spacing"
        | "line_spacing"
        | "line_spacing_multiple"
        | "indent_level"
        | "baseline"
        | "para_space_before"
        | "para_space_after"
        | "inset"
        | "line_size"
        | "pt"
        | "size"
        | "width"
        | "indent"
        | "number_start_at"
        | "margin_pt"
        | "start_at"
        | "slide" => num(),
        "color" | "highlight" => Kind::Color,
        "font_face" | "lang" | "path" | "data" | "object_name" | "shape_name" | "alt_text"
        | "character_code" | "code" | "url" | "tooltip" | "placeholder" | "src" | "rounding" => {
            Kind::Str
        }
        "x" | "y" | "w" | "h" => Kind::Coord,
        "margin" => Kind::Margin,
        "points" => Kind::Points,
        "tab_stops" => Kind::TabStops,
        "fill" => Kind::ObjOrStr(fill_schema()),
        "line" => Kind::ObjOrStr(line_schema()),
        "shadow" => Kind::Tables(shadow_schema()),
        "sizing" => Kind::Tables(sizing_schema()),
        "hyperlink" => Kind::Tables(hyperlink_schema()),
        "outline" => Kind::Tables(outline_schema()),
        "glow" => Kind::Tables(glow_schema()),
        "bullet" => Kind::BoolOrObj(bullet_schema()),
        "underline" => Kind::BoolOrStrOrObj(underline_schema()),
        _ => return None,
    };
    Some(k)
}

fn number_value(v: &toml::Value) -> Option<f64> {
    match v {
        toml::Value::Integer(i) => Some(*i as f64),
        toml::Value::Float(f) => Some(*f),
        _ => None,
    }
}

fn is_color(s: &str) -> bool {
    let h = s.strip_prefix('#').unwrap_or(s);
    (h.len() == 6 && h.chars().all(|c| c.is_ascii_hexdigit())) || THEME_COLORS.contains(&s)
}

fn check_value(kind: Kind, key: &str, value: &toml::Value, where_: &str) -> Result<()> {
    use toml::Value as V;
    macro_rules! bad {
        ($msg:expr) => {
            return Err(miette!(
                "{where_}: `{key}` {msg}, got `{value}`",
                msg = $msg
            ))
        };
    }
    match kind {
        Kind::Any => {}
        Kind::Bool => {
            if !value.is_bool() {
                bad!("must be a boolean");
            }
        }
        Kind::Num(lo, hi) => match number_value(value) {
            Some(n) if n >= lo && n <= hi => {}
            Some(n) => {
                return Err(miette!(
                    "{where_}: `{key}` must be a number in [{lo}, {hi}], got `{n}`"
                ));
            }
            None if lo.is_finite() || hi.is_finite() => {
                return Err(miette!(
                    "{where_}: `{key}` must be a number in [{lo}, {hi}], got `{value}`"
                ));
            }
            None => bad!("must be a number"),
        },
        Kind::Str => {
            if !value.is_str() {
                bad!("must be a string");
            }
        }
        Kind::Color => {
            if let Some(s) = value.as_str() {
                if !is_color(s) {
                    return Err(miette!(
                        "{where_}: `{key}` must be a 6-digit hex color or a theme color (tx1, accent1, ...)"
                    ));
                }
            } else {
                bad!("must be a color string");
            }
        }
        Kind::Enum(choices) => {
            if let Some(s) = value.as_str() {
                if !choices.contains(&s) {
                    return Err(miette!(
                        "{where_}: `{key}` must be one of {}, got `{s}`",
                        choices.join("|")
                    ));
                }
            } else {
                bad!("must be one of {}".replace("{}", &choices.join("|")));
            }
        }
        Kind::ShapePreset => {
            if let Some(s) = value.as_str() {
                if canonical_preset(s).is_none() {
                    return Err(miette!(
                        "{where_}: `{key}` is not a known shape preset, got `{s}`"
                    ));
                }
            } else {
                bad!("must be a shape preset id");
            }
        }
        Kind::Coord => match value {
            V::Integer(_) | V::Float(_) => {}
            V::String(s) => {
                if crate::units::Coord::Text(s.clone()).emu(0).is_err() {
                    return Err(miette!(
                        "{where_}: `{key}` must be an EMU integer, a unit string (\"1cm\") or a percentage"
                    ));
                }
            }
            _ => bad!("must be a coordinate"),
        },
        Kind::Margin => match value {
            V::Integer(_) | V::Float(_) => {}
            V::String(s) => {
                if crate::units::Coord::Text(s.to_string()).inches(0).is_err() {
                    return Err(miette!(
                        "{where_}: `{key}` must be a number, a length string or a percentage"
                    ));
                }
            }
            V::Array(items) if items.len() == 4 => {
                for i in items {
                    check_value(Kind::Coord, key, i, where_)?;
                }
            }
            _ => bad!("must be a number, a length, or a 4-value margin"),
        },
        Kind::Num2(lo, hi) => {
            if let Some(items) = value.as_array()
                && items.len() == 2
            {
                for i in items {
                    match number_value(i) {
                        Some(n) if n >= lo && n <= hi => {}
                        _ => {
                            bad!("must be two numbers in [{lo}, {hi}]");
                        }
                    }
                }
            } else {
                bad!("must be a two-number array");
            }
        }
        Kind::Tables(schema) => match value {
            V::Table(t) => validate_object(schema, t, key, where_)?,
            _ => bad!("must be a table"),
        },
        Kind::ObjOrStr(schema) => match value {
            V::Table(t) => validate_object(schema, t, key, where_)?,
            V::String(_) => {}
            _ => bad!("must be a table or a string shorthand"),
        },
        Kind::BoolOrObj(schema) => match value {
            V::Boolean(_) => {}
            V::Table(t) => validate_object(schema, t, key, where_)?,
            _ => bad!("must be a boolean or a table"),
        },
        Kind::BoolOrStrOrObj(schema) => match value {
            V::Boolean(_) | V::String(_) => {}
            V::Table(t) => validate_object(schema, t, key, where_)?,
            _ => bad!("must be a boolean, a string or a table"),
        },
        Kind::TabStops => {
            let items = value
                .as_array()
                .ok_or_else(|| miette!("{where_}: `{key}` must be an array of tables"))?;
            for v in items {
                let t = v
                    .as_table()
                    .ok_or_else(|| miette!("{where_}: `{key}` entries must be tables"))?;
                for (k, val) in t {
                    match k.as_str() {
                        "position" => check_value(Kind::Coord, "position", val, where_)?,
                        "alignment" => {
                            check_value(Kind::Enum(TAB_ALIGN), "alignment", val, where_)?
                        }
                        _ => return Err(miette!("{where_}: unknown `{key}` tab stop field `{k}`")),
                    }
                }
            }
        }
        Kind::Points => {
            let items = value
                .as_array()
                .ok_or_else(|| miette!("{where_}: `{key}` must be an array of tables"))?;
            for v in items {
                let t = v
                    .as_table()
                    .ok_or_else(|| miette!("{where_}: `{key}` entries must be tables"))?;
                for (k, val) in t {
                    match k.as_str() {
                        "x" | "y" | "hR" | "wR" | "x1" | "y1" | "x2" | "y2" => {
                            check_value(Kind::Coord, k, val, where_)?
                        }
                        "moveTo" | "close" => check_value(Kind::Bool, k, val, where_)?,
                        "stAng" | "swAng" => check_value(Kind::Num(-360.0, 360.0), k, val, where_)?,
                        "curve" => {
                            let ct = val
                                .as_table()
                                .ok_or_else(|| miette!("{where_}: `curve` must be a table"))?;
                            for (ck, cv) in ct {
                                match ck.as_str() {
                                    "type" => {
                                        check_value(Kind::Enum(CURVE_TYPE), "type", cv, where_)?
                                    }
                                    "hR" | "wR" | "x1" | "y1" | "x2" | "y2" => {
                                        check_value(Kind::Coord, ck, cv, where_)?
                                    }
                                    "stAng" | "swAng" => {
                                        check_value(Kind::Num(-360.0, 360.0), ck, cv, where_)?
                                    }
                                    _ => {
                                        return Err(miette!(
                                            "{where_}: unknown `points` curve field `{ck}`"
                                        ));
                                    }
                                }
                            }
                        }
                        _ => return Err(miette!("{where_}: unknown `{key}` point field `{k}`")),
                    }
                }
            }
        }
        Kind::ArrayOfTables => {
            let items = value
                .as_array()
                .ok_or_else(|| miette!("{where_}: `{key}` must be an array of tables"))?;
            for v in items {
                if !v.is_table() {
                    bad!("entries must be tables");
                }
            }
        }
    }
    Ok(())
}

fn validate_object(
    schema: &[(&'static str, Kind)],
    table: &toml::Table,
    object: &str,
    where_: &str,
) -> Result<()> {
    for (k, v) in table {
        let kind = schema
            .iter()
            .find(|(name, _)| *name == k)
            .map(|(_, k)| *k)
            .ok_or_else(|| miette!("{where_}: unknown `{object}` option `{k}`"))?;
        check_value(kind, k, v, where_)?;
    }
    Ok(())
}

/// Ordered-list marker display patterns mapped to pptxgenjs `buAutoNum`
/// number types. The pattern's counter char is `1`, `a`, `A`, `i` or `I`.
pub const MARKER_PATTERNS: &[(&str, &str)] = &[
    ("1", "arabicPlain"),
    ("1.", "arabicPeriod"),
    ("1)", "arabicParenR"),
    ("(1)", "arabicParenBoth"),
    ("a.", "alphaLcPeriod"),
    ("a)", "alphaLcParenR"),
    ("(a)", "alphaLcParenBoth"),
    ("A.", "alphaUcPeriod"),
    ("A)", "alphaUcParenR"),
    ("(A)", "alphaUcParenBoth"),
    ("i.", "romanLcPeriod"),
    ("i)", "romanLcParenR"),
    ("(i)", "romanLcParenBoth"),
    ("I.", "romanUcPeriod"),
    ("I)", "romanUcParenR"),
    ("(I)", "romanUcParenBoth"),
];

/// Parse a `styles` value: a scalar string or a list of strings.
pub fn parse_styles_value(v: &toml::Value) -> Option<Vec<String>> {
    match v {
        toml::Value::String(s) => Some(vec![s.clone()]),
        toml::Value::Array(items) => {
            let mut out = Vec::new();
            for i in items {
                out.push(i.as_str()?.to_string());
            }
            Some(out)
        }
        _ => None,
    }
}

/// Validate the gwen-owned list-marker options inside a style bucket:
/// `ordered_markers` must be display patterns, `unordered_markers` must be
/// single unicode codepoints (converted to bullet character codes by gwen).
pub fn validate_list_markers(table: &toml::Table, where_: &str) -> Result<()> {
    if let Some(arr) = table.get("ordered_markers") {
        let arr = arr.as_array().ok_or_else(|| {
            miette!("{where_}: `ordered_markers` must be an array of marker patterns")
        })?;
        for v in arr {
            let s = v
                .as_str()
                .ok_or_else(|| miette!("{where_}: `ordered_markers` entries must be strings"))?;
            if !MARKER_PATTERNS.iter().any(|(p, _)| *p == s) {
                return Err(miette!(
                    "{where_}: unknown ordered marker `{s}` (use e.g. \"1.\", \"(1)\", \"A.\")"
                ));
            }
        }
    }
    if let Some(arr) = table.get("unordered_markers") {
        let arr = arr.as_array().ok_or_else(|| {
            miette!("{where_}: `unordered_markers` must be an array of bullet characters")
        })?;
        for v in arr {
            let s = v
                .as_str()
                .ok_or_else(|| miette!("{where_}: `unordered_markers` entries must be strings"))?;
            if s.chars().count() != 1 {
                return Err(miette!(
                    "{where_}: `unordered_markers` entry `{s}` must be a single character"
                ));
            }
        }
    }
    Ok(())
}

/// Located version of `validate_list_markers`, over a source toml_edit table.
pub fn validate_list_markers_edit(table: &toml_edit::Table, src: &Src, where_: &str) -> Result<()> {
    if let Some(item) = table.get("ordered_markers") {
        let Some(arr) = item.as_value().and_then(Value::as_array) else {
            return Err(located(
                item.span(),
                src,
                format!("{where_}: `ordered_markers` must be an array of marker patterns"),
            ));
        };
        for v in arr.iter() {
            let s = v.as_str().ok_or_else(|| {
                located(
                    v.span(),
                    src,
                    format!("{where_}: `ordered_markers` entries must be strings"),
                )
            })?;
            if !MARKER_PATTERNS.iter().any(|(p, _)| *p == s) {
                return Err(located(
                    v.span(),
                    src,
                    format!(
                        "{where_}: unknown ordered marker `{s}` (use e.g. \"1.\", \"(1)\", \"A.\")"
                    ),
                ));
            }
        }
    }
    if let Some(item) = table.get("unordered_markers") {
        let Some(arr) = item.as_value().and_then(Value::as_array) else {
            return Err(located(
                item.span(),
                src,
                format!("{where_}: `unordered_markers` must be an array of bullet characters"),
            ));
        };
        for v in arr.iter() {
            let s = v.as_str().ok_or_else(|| {
                located(
                    v.span(),
                    src,
                    format!("{where_}: `unordered_markers` entries must be strings"),
                )
            })?;
            if s.chars().count() != 1 {
                return Err(located(
                    v.span(),
                    src,
                    format!("{where_}: `unordered_markers` entry `{s}` must be a single character"),
                ));
            }
        }
    }
    Ok(())
}

/// Validate an option map. A non-table value (e.g. a color shorthand for
/// `background`) is fine; only table keys and their values are checked.
pub fn validate_ctx(ctx: Ctx, value: &toml::Value, where_: &str) -> Result<()> {
    let Some(table) = value.as_table() else {
        return Ok(());
    };
    let (keys, label, extra) = ctx_parts(ctx);
    let mut keys: Vec<&str> = keys.to_vec();
    keys.extend(POSITION);
    keys.extend(extra);
    for (k, v) in table {
        if !keys.contains(&k.as_str()) {
            return Err(miette!(
                "{where_}: unknown {label} option `{k}` (not a pptxgenjs option)"
            ));
        }
        if let Some(kind) = kind_for(ctx, k) {
            check_value(kind, k, v, where_)?;
        }
    }
    Ok(())
}

fn ctx_parts(
    ctx: Ctx,
) -> (
    &'static [&'static str],
    &'static str,
    &'static [&'static str],
) {
    match ctx {
        Ctx::Text => (TEXT_KEYS, "text", &[]),
        Ctx::Placeholder => (TEXT_KEYS, "placeholder", &["name", "ph_type"]),
        Ctx::TextShape => (TEXTSHAPE_KEYS, "shape", &[]),
        Ctx::Image => (IMAGE_KEYS, "image", &[]),
        Ctx::Background => (BACKGROUND_KEYS, "background", &[]),
        Ctx::Style => (STYLE_KEYS, "style", &[]),
    }
}

/// A parsed source TOML file, used to render diagnostics that point at the
/// exact `path:line:col` of a misconfiguration.
pub struct Src {
    pub path: String,
    pub text: String,
}

/// Build a located miette diagnostic. When a byte span into `src` is known the
/// error carries the source file plus a label; otherwise it degrades to the
/// plain `where_` message.
fn located(span: Option<std::ops::Range<usize>>, src: &Src, msg: String) -> miette::Report {
    let mut diag = miette::MietteDiagnostic::new(msg);
    if let Some(sp) = span {
        diag = diag.and_label(miette::LabeledSpan::at(sp, "here"));
        return miette::Report::new(diag)
            .with_source_code(miette::NamedSource::new(src.path.clone(), src.text.clone()));
    }
    miette::Report::new(diag)
}

/// Validate the option map of a single source table (a shape, a paragraph, a
/// style bucket, ...) against the effective pptxgenjs context, producing
/// diagnostics that carry the exact `path:line:col` of a bad key or value.
/// `skip` names the typed keys that live outside the option map (`type`,
/// `styles`, `text`, `src`, `x`, `y`, `w`, `h`, `paragraphs`).
pub fn validate_source(
    ctx: Ctx,
    table: &toml_edit::Table,
    src: &Src,
    where_: &str,
    skip: &[&str],
) -> Result<()> {
    let (keys, label, extra) = ctx_parts(ctx);
    let mut keys: Vec<&str> = keys.to_vec();
    keys.extend(POSITION);
    keys.extend(extra);
    for (k, item) in table.iter() {
        if skip.contains(&k) {
            continue;
        }
        let Some(v) = item.as_value() else {
            continue;
        };
        if !keys.contains(&k) {
            return Err(located(
                item.span(),
                src,
                format!("{where_}: unknown {label} option `{k}` (not a pptxgenjs option)"),
            ));
        }
        if let Some(kind) = kind_for(ctx, k) {
            check_value_edit(kind, k, v, src, where_)?;
        }
    }
    Ok(())
}

fn check_value_edit(
    kind: Kind,
    key: &str,
    value: &toml_edit::Value,
    src: &Src,
    where_: &str,
) -> Result<()> {
    use toml_edit::Value as V;
    macro_rules! bad {
        ($msg:expr) => {
            return Err(located(
                value.span(),
                src,
                format!("{where_}: `{key}` {msg}, got `{value}`", msg = $msg),
            ))
        };
    }
    match kind {
        Kind::Any => {}
        Kind::Bool => {
            if !value.is_bool() {
                bad!("must be a boolean");
            }
        }
        Kind::Num(lo, hi) => match number_value_edit(value) {
            Some(n) if n >= lo && n <= hi => {}
            Some(n) => {
                return Err(located(
                    value.span(),
                    src,
                    format!("{where_}: `{key}` must be a number in [{lo}, {hi}], got `{n}`"),
                ));
            }
            None if lo.is_finite() || hi.is_finite() => {
                return Err(located(
                    value.span(),
                    src,
                    format!("{where_}: `{key}` must be a number in [{lo}, {hi}], got `{value}`"),
                ));
            }
            None => bad!("must be a number"),
        },
        Kind::Str => {
            if !value.is_str() {
                bad!("must be a string");
            }
        }
        Kind::Color => {
            if let Some(s) = value.as_str() {
                if !is_color(s) {
                    return Err(located(
                        value.span(),
                        src,
                        format!(
                            "{where_}: `{key}` must be a 6-digit hex color or a theme color (tx1, accent1, ...)"
                        ),
                    ));
                }
            } else {
                bad!("must be a color string");
            }
        }
        Kind::Enum(choices) => {
            if let Some(s) = value.as_str() {
                if !choices.contains(&s) {
                    return Err(located(
                        value.span(),
                        src,
                        format!(
                            "{where_}: `{key}` must be one of {}, got `{s}`",
                            choices.join("|")
                        ),
                    ));
                }
            } else {
                bad!("must be one of {}".replace("{}", &choices.join("|")));
            }
        }
        Kind::ShapePreset => {
            if let Some(s) = value.as_str() {
                if canonical_preset(s).is_none() {
                    return Err(located(
                        value.span(),
                        src,
                        format!("{where_}: `{key}` is not a known shape preset, got `{s}`"),
                    ));
                }
            } else {
                bad!("must be a shape preset id");
            }
        }
        Kind::Coord => match value {
            V::Integer(_) | V::Float(_) => {}
            V::String(s) => {
                if crate::units::Coord::Text(s.value().clone()).emu(0).is_err() {
                    return Err(located(
                        value.span(),
                        src,
                        format!(
                            "{where_}: `{key}` must be a number, a length string (\"1cm\") or a percentage"
                        ),
                    ));
                }
            }
            _ => bad!("must be a coordinate"),
        },
        Kind::Margin => match value {
            V::Integer(_) | V::Float(_) => {}
            V::String(s) => {
                if crate::units::Coord::Text(s.value().clone())
                    .inches(0)
                    .is_err()
                {
                    return Err(located(
                        value.span(),
                        src,
                        format!(
                            "{where_}: `{key}` must be a number, a length string or a percentage"
                        ),
                    ));
                }
            }
            V::Array(items) if items.len() == 4 => {
                for i in items {
                    check_value_edit(Kind::Coord, key, i, src, where_)?;
                }
            }
            _ => bad!("must be a number, a length, or a 4-value margin"),
        },
        Kind::Num2(lo, hi) => {
            if let Some(items) = value.as_array()
                && items.len() == 2
            {
                for i in items {
                    match number_value_edit(i) {
                        Some(n) if n >= lo && n <= hi => {}
                        _ => {
                            bad!("must be two numbers in [{lo}, {hi}]");
                        }
                    }
                }
            } else {
                bad!("must be a two-number array");
            }
        }
        Kind::Tables(schema) => match value {
            V::InlineTable(t) => validate_object_edit(schema, t, key, src, where_)?,
            _ => bad!("must be a table"),
        },
        Kind::ObjOrStr(schema) => match value {
            V::InlineTable(t) => validate_object_edit(schema, t, key, src, where_)?,
            V::String(_) => {}
            _ => bad!("must be a table or a string shorthand"),
        },
        Kind::BoolOrObj(schema) => match value {
            V::Boolean(_) => {}
            V::InlineTable(t) => validate_object_edit(schema, t, key, src, where_)?,
            _ => bad!("must be a boolean or a table"),
        },
        Kind::BoolOrStrOrObj(schema) => match value {
            V::Boolean(_) | V::String(_) => {}
            V::InlineTable(t) => validate_object_edit(schema, t, key, src, where_)?,
            _ => bad!("must be a boolean, a string or a table"),
        },
        Kind::TabStops => {
            let items = value.as_array().ok_or_else(|| {
                located(
                    value.span(),
                    src,
                    format!("{where_}: `{key}` must be an array of tables"),
                )
            })?;
            for v in items {
                let t = v.as_inline_table().ok_or_else(|| {
                    located(
                        v.span(),
                        src,
                        format!("{where_}: `{key}` entries must be tables"),
                    )
                })?;
                for (k, val) in t.iter() {
                    match k {
                        "position" => check_value_edit(Kind::Coord, "position", val, src, where_)?,
                        "alignment" => {
                            check_value_edit(Kind::Enum(TAB_ALIGN), "alignment", val, src, where_)?
                        }
                        _ => {
                            return Err(located(
                                val.span(),
                                src,
                                format!("{where_}: unknown `{key}` tab stop field `{k}`"),
                            ));
                        }
                    }
                }
            }
        }
        Kind::Points => {
            let items = value.as_array().ok_or_else(|| {
                located(
                    value.span(),
                    src,
                    format!("{where_}: `{key}` must be an array of tables"),
                )
            })?;
            for v in items {
                let t = v.as_inline_table().ok_or_else(|| {
                    located(
                        v.span(),
                        src,
                        format!("{where_}: `{key}` entries must be tables"),
                    )
                })?;
                for (k, val) in t.iter() {
                    match k {
                        "x" | "y" | "hR" | "wR" | "x1" | "y1" | "x2" | "y2" => {
                            check_value_edit(Kind::Coord, k, val, src, where_)?
                        }
                        "moveTo" | "close" => check_value_edit(Kind::Bool, k, val, src, where_)?,
                        "stAng" | "swAng" => {
                            check_value_edit(Kind::Num(-360.0, 360.0), k, val, src, where_)?
                        }
                        "curve" => {
                            let ct = val.as_inline_table().ok_or_else(|| {
                                located(
                                    val.span(),
                                    src,
                                    format!("{where_}: `curve` must be a table"),
                                )
                            })?;
                            for (ck, cv) in ct.iter() {
                                match ck {
                                    "type" => check_value_edit(
                                        Kind::Enum(CURVE_TYPE),
                                        "type",
                                        cv,
                                        src,
                                        where_,
                                    )?,
                                    "hR" | "wR" | "x1" | "y1" | "x2" | "y2" => {
                                        check_value_edit(Kind::Coord, ck, cv, src, where_)?
                                    }
                                    "stAng" | "swAng" => check_value_edit(
                                        Kind::Num(-360.0, 360.0),
                                        ck,
                                        cv,
                                        src,
                                        where_,
                                    )?,
                                    _ => {
                                        return Err(located(
                                            cv.span(),
                                            src,
                                            format!(
                                                "{where_}: unknown `points` curve field `{ck}`"
                                            ),
                                        ));
                                    }
                                }
                            }
                        }
                        _ => {
                            return Err(located(
                                val.span(),
                                src,
                                format!("{where_}: unknown `{key}` point field `{k}`"),
                            ));
                        }
                    }
                }
            }
        }
        Kind::ArrayOfTables => {
            let items = value.as_array().ok_or_else(|| {
                located(
                    value.span(),
                    src,
                    format!("{where_}: `{key}` must be an array of tables"),
                )
            })?;
            for v in items {
                if !v.is_inline_table() {
                    bad!("entries must be tables");
                }
            }
        }
    }
    Ok(())
}

fn validate_object_edit(
    schema: &[(&'static str, Kind)],
    table: &toml_edit::InlineTable,
    object: &str,
    src: &Src,
    where_: &str,
) -> Result<()> {
    for (k, v) in table.iter() {
        let kind = schema
            .iter()
            .find(|(name, _)| *name == k)
            .map(|(_, k)| *k)
            .ok_or_else(|| {
                located(
                    v.span(),
                    src,
                    format!("{where_}: unknown `{object}` option `{k}`"),
                )
            })?;
        check_value_edit(kind, k, v, src, where_)?;
    }
    Ok(())
}

fn number_value_edit(v: &toml_edit::Value) -> Option<f64> {
    match v {
        toml_edit::Value::Integer(i) => Some(*i.value() as f64),
        toml_edit::Value::Float(f) => Some(*f.value()),
        _ => None,
    }
}

/// Convert coordinate option values into the units pptxgenjs expects and
/// canonicalise `shape` preset ids. Runs after validation, over a merged
/// option table: bare numbers are already inches (pptxgenjs-native) and `%`
/// pass through, so only unit-length strings (`"0.13cm"`, `"1in"`, ...) are
/// converted. `shape = "round_rect"` becomes `roundRect`.
pub fn finalize_opts(ctx: Ctx, value: &mut toml::Value) -> Result<()> {
    let Some(table) = value.as_table_mut() else {
        return Ok(());
    };
    let keys: Vec<String> = table.keys().cloned().collect();
    for k in keys {
        if k == "shape"
            && let Some(s) = table.get_mut(&k).and_then(|v| v.as_str())
            && let Some(canon) = canonical_preset(s)
        {
            table.insert(k.clone(), toml::Value::String(canon.to_string()));
        }
        let Some(item) = table.get_mut(&k) else {
            continue;
        };
        let Some(kind) = kind_for(ctx, &k) else {
            continue;
        };
        match kind {
            Kind::Coord => coerce_len(item),
            Kind::Margin => coerce_margin(item),
            Kind::Tables(schema)
            | Kind::ObjOrStr(schema)
            | Kind::BoolOrObj(schema)
            | Kind::BoolOrStrOrObj(schema) => {
                if let Some(t) = item.as_table_mut() {
                    for (name, ikind) in schema {
                        if matches!(ikind, Kind::Coord)
                            && let Some(iv) = t.get_mut(*name)
                        {
                            coerce_len(iv);
                        }
                    }
                }
            }
            Kind::TabStops => {
                if let Some(items) = item.as_array_mut() {
                    for it in items.iter_mut() {
                        if let Some(t) = it.as_table_mut()
                            && let Some(p) = t.get_mut("position")
                        {
                            coerce_len(p);
                        }
                    }
                }
            }
            Kind::Points => {
                if let Some(items) = item.as_array_mut() {
                    for it in items.iter_mut() {
                        let Some(t) = it.as_table_mut() else {
                            continue;
                        };
                        let keys: Vec<String> = t.keys().cloned().collect();
                        for pk in keys {
                            if matches!(
                                pk.as_str(),
                                "x" | "y" | "hR" | "wR" | "x1" | "y1" | "x2" | "y2"
                            ) {
                                if let Some(pv) = t.get_mut(&pk) {
                                    coerce_len(pv);
                                }
                            } else if pk == "curve"
                                && let Some(ct) = t.get_mut("curve").and_then(|c| c.as_table_mut())
                            {
                                for ck in ct.keys().cloned().collect::<Vec<_>>() {
                                    if !matches!(ck.as_str(), "type" | "stAng" | "swAng")
                                        && let Some(cv) = ct.get_mut(&ck)
                                    {
                                        coerce_len(cv);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn coerce_len(v: &mut toml::Value) {
    if let toml::Value::String(s) = v {
        if s.trim_end().ends_with('%') {
            return;
        }
        if let Ok(inches) = crate::units::Coord::Text(s.clone()).inches(0) {
            *v = toml::Value::Float(inches);
        }
    }
}

fn coerce_margin(v: &mut toml::Value) {
    if let toml::Value::Array(items) = v {
        for i in items {
            coerce_len(i);
        }
    } else {
        coerce_len(v);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(s: &str) -> toml::Table {
        toml::from_str(s).unwrap()
    }

    fn ctx(ctx: Ctx, s: &str) -> miette::Result<()> {
        validate_ctx(ctx, &toml::Value::Table(table(s)), "x")
    }

    #[test]
    fn known_options_pass() {
        ctx(
            Ctx::Text,
            "font_size = 14\nbold = true\nalign = \"center\"\nfill = { color = \"C7000A\" }\nmargin = [0.1, 0.1, 0.1, 0.1]\ntext_direction = \"vert\"\n",
        )
        .unwrap();
    }

    #[test]
    fn unknown_option_errors() {
        let err = ctx(Ctx::Text, "fount_size = 14\n").unwrap_err();
        assert!(err.to_string().contains("fount_size"));
    }

    #[test]
    fn unknown_nested_option_errors() {
        let err = ctx(Ctx::Text, "fill = { colro = \"C7000A\" }\n").unwrap_err();
        assert!(err.to_string().contains("colro"));
    }

    #[test]
    fn string_shorthand_for_nested_allowed() {
        ctx(Ctx::Text, "fill = \"FF0000\"\n").unwrap();
    }

    #[test]
    fn placeholder_allows_name_and_ph_type() {
        ctx(
            Ctx::Placeholder,
            "name = \"Title\"\nph_type = \"title\"\nfont_size = 12\n",
        )
        .unwrap();
    }

    #[test]
    fn style_rejects_unknown_keys() {
        ctx(Ctx::Style, "arc_thickness_ratio = 0.5\n").unwrap();
        assert!(ctx(Ctx::Style, "nonsense = true\n").is_err());
    }

    #[test]
    fn wrong_value_types_error() {
        for (s, needle) in [
            ("rect_radius = \"1cm\"\n", "must be a number in [0, 1]"),
            ("font_size = \"big\"\n", "must be a number"),
            ("transparency = \"half\"\n", "must be a number"),
            ("bold = \"yes\"\n", "must be a boolean"),
            ("bullet = \"yes\"\n", "must be a boolean or a table"),
            (
                "align = \"centr\"\n",
                "must be one of left|center|right|justify",
            ),
            ("fit = \"stretch\"\n", "must be one of"),
            (
                "color = \"xyz\"\n",
                "must be a 6-digit hex color or a theme color",
            ),
            (
                "color = \"zzzzzz\"\n",
                "must be a 6-digit hex color or a theme color",
            ),
            (
                "margin = [1, 2, 3, 4, 5]\n",
                "must be a number, a length, or a 4-value margin",
            ),
            (
                "margin = \"nope\"\n",
                "must be a number, a length string or a percentage",
            ),
            ("angle_range = [1, 2, 3]\n", "must be a two-number array"),
            (
                "shadow = { offset = 999 }\n",
                "must be a number in [0, 200]",
            ),
            (
                "sizing = { type = \"zoom\" }\n",
                "must be one of contain|cover|crop",
            ),
            ("rotate = 500\n", "must be a number in [-360, 360]"),
            ("line = { dash_type = \"wavy\" }\n", "dash_type"),
        ] {
            let err = ctx(Ctx::TextShape, s).unwrap_err();
            assert!(
                err.to_string().contains(needle),
                "expected `{needle}` in error for {s:?}, got: {err:?}"
            );
        }
    }

    #[test]
    fn value_union_types_pass() {
        for s in [
            "x = 1000\n",
            "x = \"1cm\"\n",
            "sizing = { w = \"75%\", type = \"contain\" }\n",
            "color = \"#FF0000\"\n",
            "color = \"accent1\"\n",
            "fill = \"FFFF00\"\n",
            "fill = { color = \"00AAFF\" }\n",
            "margin = [0.1, 0.1, 0.1, 0.1]\n",
            "margin = [\"0.13cm\", \"0.25cm\", \"0.13cm\", \"0.25cm\"]\n",
            "margin = \"0.13cm\"\n",
            "bullet = false\n",
            "bullet = { type = \"number\", number_type = \"arabicPeriod\" }\n",
            "underline = \"sng\"\n",
            "rotate = 180\n",
            "transparency = 100\n",
            "rect_radius = 1\n",
        ] {
            ctx(Ctx::TextShape, s).unwrap_or_else(|e| panic!("should pass `{s}`: {e:?}"));
        }
        ctx(
            Ctx::TextShape,
            "points = [{ x = 0, y = 0 }, { close = true }]\n",
        )
        .unwrap();
        ctx(
            Ctx::Text,
            "tab_stops = [{ position = 1, alignment = \"ctr\" }]\n",
        )
        .unwrap();
        // Number-currently snake names must be accepted by canonical_preset.
        ctx(Ctx::Text, "shape = \"round_rect\"\n").unwrap();
    }

    #[test]
    fn coord_and_enum_reject_bad_values() {
        let err = ctx(Ctx::Text, "x = \"nope\"\n").unwrap_err();
        assert!(err.to_string().contains("EMU integer"));
        let err = ctx(Ctx::Text, "shape = \"rectt\"\n").unwrap_err();
        assert!(err.to_string().contains("not a known shape preset"));
    }

    #[test]
    fn finalize_converts_coords_and_canonicalises_shape() {
        let t = toml::from_str::<toml::Table>(
            "margin = [\"0.13cm\", \"0.25cm\", \"0.13cm\", \"0.25cm\"]\n\
             sizing = { w = \"1cm\", h = \"75%\", type = \"contain\" }\n\
             tab_stops = [{ position = \"1in\", alignment = \"ctr\" }]\n\
             points = [{ x = \"1cm\", y = 1, close = false }]\n\
             shape = \"round_rect\"\n",
        )
        .unwrap();
        let mut value = toml::Value::Table(t);
        finalize_opts(Ctx::TextShape, &mut value).unwrap();
        let t = value.as_table().unwrap();

        // Unit lengths are converted to inches; numbers stay as native inches.
        if let Some(toml::Value::Array(items)) = t.get("margin") {
            let vals: Vec<f64> = items
                .iter()
                .map(|v| {
                    v.as_float()
                        .unwrap_or_else(|| v.as_integer().unwrap() as f64)
                })
                .collect();
            for (got, want) in vals.iter().zip([0.0512, 0.0984, 0.0512, 0.0984]) {
                assert!((got - want).abs() < 0.001, "margin entry {got} != ~{want}");
            }
        } else {
            panic!("margin should be coerceable");
        }
        // `%` passes through untouched.
        assert_eq!(t["sizing"]["h"].as_str(), Some("75%"));
        assert!((t["sizing"]["w"].as_float().unwrap() - 0.3937).abs() < 0.001);
        // tab_stops.position inches.
        assert!((t["tab_stops"][0]["position"].as_float().unwrap() - 1.0).abs() < 1e-9);
        // points coords inches; numbers and booleans untouched.
        assert!((t["points"][0]["x"].as_float().unwrap() - 0.3937).abs() < 0.001);
        assert_eq!(t["points"][0]["y"].as_integer(), Some(1));
        assert_eq!(t["points"][0]["close"].as_bool(), Some(false));
        // shape snake_case canonicalised.
        assert_eq!(t["shape"].as_str(), Some("roundRect"));
    }

    #[test]
    fn located_errors_carry_file_and_span() {
        let text = "[[shapes]]\ntype = \"text\"\ntext = \"hi\"\nmargin = [\"nope\", \"0.25cm\", \"0.13cm\", \"0.25cm\"]\n";
        let doc: toml_edit::Document<String> = text.parse().unwrap();
        let table = doc
            .as_table()
            .get("shapes")
            .and_then(|i| i.as_array_of_tables())
            .and_then(|a| a.get(0))
            .cloned()
            .unwrap();
        let src = Src {
            path: "slides/a.toml".into(),
            text: text.into(),
        };
        let err = validate_source(
            Ctx::Text,
            &table,
            &src,
            "slide `a.toml` shape `text`",
            &[
                "type",
                "styles",
                "text",
                "paragraphs",
                "src",
                "x",
                "y",
                "w",
                "h",
            ],
        )
        .unwrap_err();
        let msg = format!("{err:?}");
        assert!(
            msg.contains("slides/a.toml"),
            "diagnostic mentions the file: {msg}"
        );
        assert!(msg.contains("margin"), "diagnostic mentions the key: {msg}");
        assert!(
            msg.contains("a length string"),
            "diagnostic explains the bad value: {msg}"
        );
    }
}
