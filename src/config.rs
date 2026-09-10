//! Project configuration: presentation metadata, theme fonts, and the
//! user-defined layouts (arbitrary shapes + content slots).

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::error::Result;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub presentation: Presentation,
    #[serde(default)]
    pub theme: Theme,
    #[serde(default)]
    pub layouts: BTreeMap<String, Layout>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Presentation {
    pub name: String,
    pub slide_width: i64,
    pub slide_height: i64,
    pub default_layout: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Theme {
    #[serde(default = "default_font")]
    pub major_font: String,
    #[serde(default = "default_font")]
    pub minor_font: String,
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            major_font: default_font(),
            minor_font: default_font(),
        }
    }
}

fn default_font() -> String {
    "Calibri".to_string()
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Layout {
    #[serde(default)]
    pub elements: Vec<Element>,
}

/// A layout element is either a decorative shape (drawn on every slide that
/// uses the layout) or a named content slot (bound by slide markdown).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Element {
    Shape(ShapeElement),
    Slot(SlotElement),
}

#[derive(Debug, Clone, Deserialize)]
pub struct ShapeElement {
    pub shape: ShapeKind,
    pub left: i64,
    pub top: i64,
    pub width: i64,
    pub height: i64,
    /// Fill color as `#RRGGBB`. Ignored when `no_fill` is set.
    #[serde(default)]
    pub fill: Option<String>,
    /// When true the shape is an outline only (no fill).
    #[serde(default)]
    pub no_fill: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapeKind {
    Rect,
    RoundRect,
    Ellipse,
    Line,
}

impl ShapeKind {
    pub fn preset(self) -> &'static str {
        match self {
            ShapeKind::Rect => "rect",
            ShapeKind::RoundRect => "roundRect",
            ShapeKind::Ellipse => "ellipse",
            ShapeKind::Line => "line",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SlotElement {
    pub slot: String,
    #[serde(rename = "type")]
    pub kind: SlotKind,
    pub left: i64,
    pub top: i64,
    pub width: i64,
    pub height: i64,
    #[serde(default)]
    pub align: Align,
    #[serde(default)]
    pub anchor: Anchor,
    /// Font size in points.
    #[serde(default)]
    pub text_size: Option<i64>,
    /// Text color as `#RRGGBB`.
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub bold: Option<bool>,
    #[serde(default)]
    pub italic: Option<bool>,
    #[serde(default)]
    pub wrap: Option<bool>,
    /// Shape fill color as `#RRGGBB` (text slots only).
    #[serde(default)]
    pub fill: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotKind {
    Text,
    Picture,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

impl Align {
    pub fn attr(self) -> &'static str {
        match self {
            Align::Left => "l",
            Align::Center => "ctr",
            Align::Right => "r",
            Align::Justify => "just",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    #[default]
    Top,
    Middle,
    Bottom,
}

impl Anchor {
    pub fn attr(self) -> &'static str {
        match self {
            Anchor::Top => "t",
            Anchor::Middle => "ctr",
            Anchor::Bottom => "b",
        }
    }
}

impl Config {
    pub fn load(project: &Path) -> Result<Config> {
        let path = project.join("config.toml");
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| miette::miette!("cannot read {}: {e}", path.display()))?;
        let config: Config =
            toml::from_str(&raw).map_err(|e| miette::miette!("invalid {}: {e}", path.display()))?;
        config.validate()?;
        let mut config = config;
        config.normalize();
        Ok(config)
    }

    /// Normalize every color to bare uppercase `RRGGBB` after validation, so
    /// the part builders never see a leading `#`.
    fn normalize(&mut self) {
        for layout in self.layouts.values_mut() {
            for element in &mut layout.elements {
                match element {
                    Element::Shape(s) => s.fill = s.fill.as_deref().map(|c| parse_hex(c).unwrap()),
                    Element::Slot(s) => {
                        s.color = s.color.as_deref().map(|c| parse_hex(c).unwrap());
                        s.fill = s.fill.as_deref().map(|c| parse_hex(c).unwrap());
                    }
                }
            }
        }
    }

    fn validate(&self) -> Result<()> {
        let p = &self.presentation;
        if p.name.trim().is_empty() {
            return Err(miette::miette!("`presentation.name` must not be empty"));
        }
        if p.slide_width <= 0 || p.slide_height <= 0 {
            return Err(miette::miette!(
                "`presentation.slide_width` and `slide_height` must be positive"
            ));
        }
        if self.layouts.is_empty() {
            return Err(miette::miette!(
                "at least one `[layouts.*]` layout is required"
            ));
        }
        if !self.layouts.contains_key(&p.default_layout) {
            return Err(miette::miette!(
                "`presentation.default_layout` is `{}`, which is not defined under `[layouts.*]`",
                p.default_layout
            ));
        }
        for (name, layout) in &self.layouts {
            let mut seen = std::collections::HashSet::new();
            for element in &layout.elements {
                match element {
                    Element::Shape(s) => {
                        validate_geometry(name, "shape", s.width, s.height)?;
                        if let Some(fill) = &s.fill {
                            parse_hex(fill)
                                .map_err(|e| miette::miette!("layout `{name}` shape fill: {e}"))?;
                        }
                    }
                    Element::Slot(s) => {
                        validate_geometry(name, &s.slot, s.width, s.height)?;
                        if !seen.insert(&s.slot) {
                            return Err(miette::miette!(
                                "layout `{name}` defines slot `{}` more than once",
                                s.slot
                            ));
                        }
                        if let Some(color) = &s.color {
                            parse_hex(color).map_err(|e| {
                                miette::miette!("layout `{name}` slot `{}` color: {e}", s.slot)
                            })?;
                        }
                        if let Some(fill) = &s.fill {
                            parse_hex(fill).map_err(|e| {
                                miette::miette!("layout `{name}` slot `{}` fill: {e}", s.slot)
                            })?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// The layout for a slide, or a diagnostic naming the unknown key.
    pub fn layout(&self, key: &str) -> Result<&Layout> {
        self.layouts.get(key).ok_or_else(|| {
            miette::miette!(
                "unknown layout `{key}` (defined: {})",
                self.layouts.keys().cloned().collect::<Vec<_>>().join(", ")
            )
        })
    }
}

fn validate_geometry(layout: &str, what: &str, width: i64, height: i64) -> Result<()> {
    if width <= 0 || height <= 0 {
        return Err(miette::miette!(
            "layout `{layout}` element `{what}` must have positive width and height"
        ));
    }
    Ok(())
}

/// Parse `#RRGGBB` (or bare `RRGGBB`) into uppercase six-hex digits.
pub fn parse_hex(value: &str) -> std::result::Result<String, String> {
    let hex = value.strip_prefix('#').unwrap_or(value);
    if hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(hex.to_ascii_uppercase())
    } else {
        Err(format!("`{value}` is not a #RRGGBB color"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r##"
[presentation]
name = "Deck"
slide_width = 12192000
slide_height = 6858000
default_layout = "content"

[theme]
major_font = "Arial Black"
minor_font = "Arial"

[[layouts.content.elements]]
kind = "shape"
shape = "round_rect"
left = 0
top = 0
width = 12192000
height = 914400
fill = "#1D1D1A"

[[layouts.content.elements]]
kind = "slot"
slot = "title"
type = "text"
left = 457200
top = 2130000
width = 10846800
height = 1371600
align = "center"
anchor = "middle"
text_size = 40
color = "#1D1D1A"
bold = true
"##;

    #[test]
    fn parses_valid_config() {
        let config: Config = toml::from_str(VALID).unwrap();
        config.validate().unwrap();
        assert_eq!(config.theme.major_font, "Arial Black");
        let layout = config.layout("content").unwrap();
        assert_eq!(layout.elements.len(), 2);
    }

    #[test]
    fn rejects_unknown_default_layout() {
        let text = VALID.replace("default_layout = \"content\"", "default_layout = \"nope\"");
        let config: Config = toml::from_str(&text).unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn rejects_bad_hex() {
        assert!(parse_hex("#GGGGGG").is_err());
        assert_eq!(parse_hex("#c7000a").unwrap(), "C7000A");
    }

    #[test]
    fn rejects_duplicate_slots() {
        let duplicated = format!(
            "{VALID}\n[[layouts.content.elements]]\nkind = \"slot\"\nslot = \"title\"\ntype = \"text\"\nleft = 0\ntop = 0\nwidth = 10\nheight = 10\n"
        );
        let config: Config = toml::from_str(&duplicated).unwrap();
        assert!(config.validate().is_err());
    }
}
