//! The self-documenting marker grammar of the markdown mirror, described in
//! `docs/mirror-format.md`. This module is the single source of truth for
//! marker keywords, marker attribute names, length unit parsing and the
//! legend every generated mirror starts with.

/// The marker comment keyword for a shape: `<!-- shape type="textbox"
/// class="textbox-1" name="..." left="..." ... -->`.
pub const MARKER_SHAPE: &str = "shape";
/// The marker comment keyword for a paragraph that deviates from its shape's
/// default paragraph style: `<!-- paragraph class="para-3" -->`.
pub const MARKER_PARAGRAPH: &str = "paragraph";
/// The marker comment keyword for a slide background:
/// `<!-- background fill="SOLID:FF0000" -->`.
pub const MARKER_BACKGROUND: &str = "background";

/// Marker attribute keys. `class` references a rule in the `<style>` block;
/// the remaining attributes are scalars carried inline.
pub const ATTR_CLASS: &str = "class";
/// `type="textbox" | "placeholder" | "picture" | "table" | "group" |
/// "chart" | "autoshape" | ...`: the shape identity (its `shape_type`).
pub const ATTR_TYPE: &str = "type";
/// `auto-shape="roundRect"`: the geometry preset for `type="autoshape"`.
pub const ATTR_AUTO_SHAPE: &str = "auto-shape";
pub const ATTR_NAME: &str = "name";
pub const ATTR_LEFT: &str = "left";
pub const ATTR_TOP: &str = "top";
pub const ATTR_WIDTH: &str = "width";
pub const ATTR_HEIGHT: &str = "height";
pub const ATTR_ROTATION: &str = "rotation";
pub const ATTR_GRID: &str = "grid";
/// `fill="SOLID:FF0000"`: the `background` marker's value.
pub const ATTR_FILL: &str = "fill";

/// The crop attribute prefix (`crop-left`, `crop-top`, `crop-right`,
/// `crop-bottom`).
pub const ATTR_CROP_PREFIX: &str = "crop-";

/// The geometry attributes that accept a length: raw EMU or a unit-suffixed
/// value (`1in`, `3cm`, `25mm`, `72pt`, `96px`).
pub const LENGTH_ATTRS: [&str; 4] = ["left", "top", "width", "height"];

/// EMU in one inch; the base unit of the OOXML geometry grid.
pub const EMU_PER_IN: i64 = 914_400;

/// Parse a geometry length: raw EMU (a bare integer) or a unit-suffixed
/// value. The mirror always writes raw EMU; units are accepted as input so
/// humans can write `width="3cm"`.
pub fn parse_length(s: &str) -> Option<i64> {
    let t = s.trim();
    for (unit, emu_per_unit) in [
        ("in", EMU_PER_IN as f64),
        ("cm", 360_000.0),
        ("mm", 36_000.0),
        ("pt", 12_700.0),
        ("px", 9_525.0), // 96 dpi
    ] {
        if let Some(num) = t.strip_suffix(unit)
            && !num.trim().is_empty()
        {
            let v: f64 = num.trim().parse().ok()?;
            return Some((v * emu_per_unit).round() as i64);
        }
    }
    t.parse().ok()
}

/// The human-readable `type=` token for a shape's `shape_type`, mirroring
/// `shape_kind`. Geometry presets are carried by the separate `auto-shape=`
/// attribute, so the token is always unambiguous.
pub fn shape_type_token(shape_type: &str) -> String {
    match shape_type {
        "TEXT_BOX" => "textbox".to_string(),
        "PLACEHOLDER" => "placeholder".to_string(),
        "PICTURE" => "picture".to_string(),
        "TABLE" => "table".to_string(),
        "GROUP" => "group".to_string(),
        "CHART" => "chart".to_string(),
        "AUTO_SHAPE" => "autoshape".to_string(),
        _ => shape_type.to_ascii_lowercase(),
    }
}

/// The inverse of `shape_type_token`: the token back to the exact
/// `shape_type` value (which also makes it safe to round-trip). An unknown
/// token is treated as a plain (possibly unknown) type name.
pub fn shape_type_from_token(token: &str) -> String {
    match token {
        "textbox" => "TEXT_BOX".to_string(),
        "placeholder" => "PLACEHOLDER".to_string(),
        "picture" => "PICTURE".to_string(),
        "table" => "TABLE".to_string(),
        "group" => "GROUP".to_string(),
        "chart" => "CHART".to_string(),
        "autoshape" => "AUTO_SHAPE".to_string(),
        _ => token.to_ascii_uppercase(),
    }
}

/// The legend emitted at the top of every mirror, after the front matter.
/// Written as a single HTML comment so the parser skips it; the marker
/// examples therefore omit the comment delimiters.
pub fn legend() -> &'static str {
    r#"<!--
  Gwen markdown mirror

  Structure
    `## Slide N`     A slide. The heading is a structural anchor; its text is
                     ignored. Add or delete slides by adding/removing these
                     headings.
    `### Notes`      The slide's notes, below its shapes.

  Shapes: one marker per shape
    shape type="textbox" class="textbox-1" name="TextBox 4" left="914400"
          top="914400" width="6096000" height="914400" rotation="-20"
          grid="775018,936978" crop-left="0.1"
    (these appear as an HTML comment)

    type="..."       textbox | placeholder | picture | table | group | chart |
                     autoshape | line | freeform | ...  the shape identity
    auto-shape="..." the geometry preset for type="autoshape", e.g. roundRect
    class="..."      the styling class in the style block below: the shape
                     fill/outline plus its frame and default paragraph style
    name, left/top/width/height, rotation, grid, crop-left/top/right/bottom

    Geometry is in EMU (914400 EMU = 1 inch). On input you may also use
    units: left="1in", width="3cm", top="25mm", height="72pt", width="96px".

  Paragraphs
    A paragraph whose own style deviates from the shape's default carries
      paragraph class="para-3"
    (an HTML comment) directly above it. Unmarked paragraphs inherit the
    shape default.

  Background
    background fill="SOLID:FF0000"

  Slide size, theme and core properties live in the YAML front matter; every
  referenced class is defined in the style block below. See
  docs/mirror-format.md for the full specification.
-->"#
}
