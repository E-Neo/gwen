//! Validate the free-form pptxgenjs option maps gwen passes through, so typos
//! like `fount_size` or a stray `transperency` inside `fill` fail at build time
//! instead of being silently ignored by pptxgenjs.
//!
//! The whitelists are in snake_case (the TOML form) and mirror the vendored
//! pptxgenjs 4.0.1 `index.d.ts`; keep them in sync with
//! `src/js/pptxgen.bundle.js` when that bundle is upgraded.

use miette::{Result, miette};

/// Which pptxgenjs option object a TOML option map feeds.
#[derive(Clone, Copy)]
pub enum Ctx {
    /// Text shapes, master text/placeholder options, paragraph options,
    /// `slide_number`.
    Text,
    /// Master placeholder options: text options plus `name`/`ph_type`.
    Placeholder,
    /// Shape presets (rect, roundRect, line, ...).
    Shape,
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

const SHAPE_KEYS: &[&str] = &[
    "align",
    "angle_range",
    "arc_thickness_ratio",
    "fill",
    "flip_h",
    "flip_v",
    "hyperlink",
    "line",
    "points",
    "rect_radius",
    "rotate",
    "shadow",
    "object_name",
    "line_size",
    "line_dash",
    "line_head",
    "line_tail",
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
        Ctx::Shape => (SHAPE_KEYS, "shape", &[]),
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
