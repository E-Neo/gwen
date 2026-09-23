//! Validate the free-form pptxgenjs option maps gwen passes through, so typos
//! like `fount_size` or a stray `transperency` inside `fill` fail at build time
//! instead of being silently ignored by pptxgenjs.
//!
//! The whitelists are in snake_case (the TOML form) and mirror the vendored
//! pptxgenjs 4.0.1 `index.d.ts`; keep them in sync with
//! `src/js/pptxgen.bundle.js` when that bundle is upgraded.

use miette::{Result, miette};

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

/// Whether a shape `type` is a valid slide shape type (`text`, `image`, or a
/// shape preset). Chart/table/media are handled separately as reserved.
pub fn is_shape_type(ty: &str) -> bool {
    matches!(ty, "text" | "image") || SHAPE_PRESETS.contains(&ty)
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

const TEXT_KEYS: &[&str] = &[
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
];

/// Shape presets can carry text (rect/ellipse/... with a `text` value), so
/// their option maps accept both shape and text keys.
const TEXTSHAPE_KEYS: &[&str] = &[
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
    "angle_range",
    "arc_thickness_ratio",
    "points",
    "shape_name",
];

const IMAGE_KEYS: &[&str] = &[
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
];

const BACKGROUND_KEYS: &[&str] = &[
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
    "angle_range",
    "arc_thickness_ratio",
    "points",
    "shape_name",
    "alt_text",
    "rounding",
    "sizing",
];

/// Nested option objects and their sub-keys (`fill`, `line`, `bullet`, ...).
const NESTED: &[(&str, &[&str])] = &[
    ("fill", &["color", "transparency", "type", "alpha"]),
    (
        "line",
        &[
            "color",
            "transparency",
            "type",
            "alpha",
            "width",
            "dash_type",
            "begin_arrow_type",
            "end_arrow_type",
            "line_dash",
            "line_head",
            "line_tail",
            "pt",
            "size",
        ],
    ),
    (
        "shadow",
        &[
            "type",
            "opacity",
            "blur",
            "angle",
            "offset",
            "color",
            "rotate_with_shape",
        ],
    ),
    (
        "bullet",
        &[
            "type",
            "character_code",
            "indent",
            "number_type",
            "number_start_at",
            "code",
            "margin_pt",
            "start_at",
            "style",
        ],
    ),
    ("sizing", &["type", "w", "h", "x", "y"]),
    ("hyperlink", &["slide", "url", "tooltip"]),
    ("underline", &["style", "color"]),
    ("outline", &["color", "size"]),
    ("glow", &["color", "opacity", "size"]),
];

/// Validate an option map. A non-table value (e.g. a color shorthand for
/// `background`) is fine; only table keys are checked.
pub fn validate_ctx(ctx: Ctx, value: &toml::Value, where_: &str) -> Result<()> {
    let Some(table) = value.as_table() else {
        return Ok(());
    };
    let (keys, label, extra): (&[&str], &str, &[&str]) = match ctx {
        Ctx::Text => (TEXT_KEYS, "text", &[]),
        Ctx::Placeholder => (TEXT_KEYS, "placeholder", &["name", "ph_type"]),
        Ctx::TextShape => (TEXTSHAPE_KEYS, "shape", &[]),
        Ctx::Image => (IMAGE_KEYS, "image", &[]),
        Ctx::Background => (BACKGROUND_KEYS, "background", &[]),
        Ctx::Style => (STYLE_KEYS, "style", &[]),
    };
    let mut keys: Vec<&str> = keys.to_vec();
    keys.extend(POSITION);
    keys.extend(extra);
    validate_table(&keys, NESTED, table, where_, label)
}

fn validate_table(
    keys: &[&str],
    nested: &[(&str, &[&str])],
    table: &toml::Table,
    where_: &str,
    label: &str,
) -> Result<()> {
    for (k, v) in table {
        if let Some((_, sub)) = nested.iter().find(|(name, _)| name == k) {
            if let Some(t) = v.as_table() {
                validate_table(sub, &[], t, where_, k)?;
            }
            continue;
        }
        if keys.contains(&k.as_str()) {
            continue;
        }
        return Err(miette!(
            "{where_}: unknown {label} option `{k}` (not a pptxgenjs option)"
        ));
    }
    Ok(())
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
}
