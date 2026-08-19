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
/// Closes a group: `<!-- end-group -->`. The shapes between the group's
/// `<!-- shape type="group" ... -->` marker and this marker are its children.
pub const MARKER_END_GROUP: &str = "end-group";
/// The marker comment keyword for a master reference in `PRESENTATION.md`:
/// `<!-- master name="..." uri="..." src="masters/master1.md" -->`.
pub const MARKER_MASTER: &str = "master";
/// The marker comment keyword for a layout reference inside a master file:
/// `<!-- layout name="..." uri="..." src="layouts/layout1.md" -->`.
pub const MARKER_LAYOUT: &str = "layout";
/// The marker comment keyword for a slide reference in `PRESENTATION.md`:
/// `<!-- slide uri="..." src="slides/slide1.md" -->`.
pub const MARKER_SLIDE: &str = "slide";

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

/// `id="4"`: the shape's unique id (required on build; new shapes may omit it).
pub const ATTR_ID: &str = "id";
/// `placeholder` (attribute with no value): `is_placeholder == true`.
pub const ATTR_PLACEHOLDER: &str = "placeholder";
pub const ATTR_PH_TYPE: &str = "ph-type";
pub const ATTR_PH_IDX: &str = "ph-idx";
pub const ATTR_PH_SZ: &str = "ph-sz";
/// `image="image1.png"`: the picture's media filename (relative to `src/media`).
pub const ATTR_IMAGE: &str = "image";
/// Group child coordinate system: `ch-off-x`, `ch-off-y`, `ch-ext-cx`,
/// `ch-ext-cy` (a:chOff / a:chExt).
pub const ATTR_CH_OFF_X: &str = "ch-off-x";
pub const ATTR_CH_OFF_Y: &str = "ch-off-y";
pub const ATTR_CH_EXT_CX: &str = "ch-ext-cx";
pub const ATTR_CH_EXT_CY: &str = "ch-ext-cy";
/// `chart-type="c:barChart"`: the chart geometry type, written on `type="chart"`
/// shapes whose series follow as a table.
pub const ATTR_CHART_TYPE: &str = "chart-type";
/// `row-heights="1000,2000,1000"`: per-row heights of the shape's table.
pub const ATTR_ROW_HEIGHTS: &str = "row-heights";
/// `merge="r,c,gridSpan,rowSpan,hMerge,vMerge;..."`: merged cells of the
/// shape's table (semicolon-separated; 0 disables a property).
pub const ATTR_MERGE: &str = "merge";
/// `uri="ppt/slides/slide1.xml"`: a master/layout/slide reference's part URI.
pub const ATTR_URI: &str = "uri";
/// `src="slides/slide1.md"`: a reference's project-relative markdown file.
pub const ATTR_SRC: &str = "src";

/// The attribute keys for a group's child coordinate system.
pub const GROUP_CH_ATTRS: [&str; 4] =
    [ATTR_CH_OFF_X, ATTR_CH_OFF_Y, ATTR_CH_EXT_CX, ATTR_CH_EXT_CY];

/// The crop attribute prefix (`crop-left`, `crop-top`, `crop-right`,
/// `crop-bottom`).
pub const ATTR_CROP_PREFIX: &str = "crop-";

/// The geometry attributes that accept a length: raw EMU or a unit-suffixed
/// value (`1in`, `3cm`, `25mm`, `72pt`, `96px`).
pub const LENGTH_ATTRS: [&str; 4] = [ATTR_LEFT, ATTR_TOP, ATTR_WIDTH, ATTR_HEIGHT];

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

  Project layout (multi-file form, `PRESENTATION.md` is the index)
    src/masters/master1.md   one slide master per file; its `# Layouts`
                             markers reference its layout files
    src/layouts/layout1.md   one slide layout per file
    src/slides/slide1.md     one slide per file; `### Notes` holds its notes
    src/parts/               every part the mirror cannot express, preserved
                             byte-for-byte (plus _rels/, _fragments/,
                             _content-types.toml)
    src/media/               picture/chart image files, keyed by filename

  Shapes: one marker per shape
    shape type="textbox" class="textbox-1" id="4" name="TextBox 4"
          left="914400" top="914400" width="6096000" height="914400"
          rotation="-20" placeholder ph-type="TITLE" ph-idx="1" ph-sz="full"
          image="image1.png" grid="775018,936978" row-heights="1000,2000"
          merge="1,0,2,1,0,0" crop-left="0.1"
          ch-off-x="0" ch-off-y="0" ch-ext-cx="0" ch-ext-cy="0"
    (these appear as an HTML comment)

    type="..."       textbox | placeholder | picture | table | group | chart |
                     autoshape | line | freeform | ...  the shape identity
    auto-shape="..." the geometry preset for type="autoshape", e.g. roundRect
    class="..."      the styling class in the style block below: the shape
                     fill/outline plus its frame and default paragraph style
    id="N"           the shape's unique id (leave out to auto-assign on build)
    placeholder      this shape is a placeholder
    ph-type/idx/sz   the placeholder's type/index/size
    image="..."      the media filename for type="picture"
    grid="..."       table column widths; row-heights="..." row heights;
                     merge="r,c,gridSpan,rowSpan,hMerge,vMerge;..." merged cells
    crop-left/top/right/bottom, ch-off-x/y, ch-ext-cx/cy

  Groups
    A `shape type="group"` marker is followed by its child shape markers and
    closes with `end-group`.

  Charts
    A `shape type="chart" chart-type="c:barChart"` marker is followed by a
    table whose header row is the categories; each later row is a series
    (name + values).

  Links
    A run may be written `[text](https://example.com)` or
    `[text](slide://2)` to link to an address or another slide.

  Paragraphs
    A paragraph whose own style deviates from the shape's default carries
      paragraph class="para-3"
    (an HTML comment) directly above it.

  Background
    background fill="SOLID:FF0000"

  Slide size, theme and core properties live in the YAML front matter; every
  referenced class is defined in the style block below. See
  docs/mirror-format.md for the full specification.
-->"#
}
