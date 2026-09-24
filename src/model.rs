//! TOML model for a gwen deck, mirroring the pptxgenjs API surface.
//!
//! `main.toml` (`[presentation]`, `[theme]`, `[[sections]]`, `[defaults.<type>]`,
//! `[styles.<name>]`), `masters/<name>.toml` and `slides/<name>.toml`. Option
//! keys are snake_case and become the corresponding pptxgenjs camelCase
//! properties; arbitrary extra keys pass straight through.

use std::collections::BTreeMap;
use std::path::PathBuf;

use miette::{Result, miette};

use crate::units::Coord;

/// A free-form option map: everything pptxgenjs accepts for a call, in TOML.
pub type Opts = toml::Table;

/// The deck root, holding parsed `main.toml` plus the file layout.
pub struct Project {
    pub dir: PathBuf,
    pub main: Main,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Main {
    pub presentation: Presentation,
    #[serde(default)]
    pub theme: Theme,
    #[serde(default)]
    pub sections: Vec<Section>,
    #[serde(default)]
    pub styles: Styles,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Presentation {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub company: String,
    #[serde(default)]
    pub revision: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub rtl_mode: bool,
    /// Slide width in EMU or a unit string/percentage.
    #[serde(default = "default_width", rename = "width")]
    pub width: Coord,
    /// Slide height in EMU or a unit string/percentage.
    #[serde(default = "default_height", rename = "height")]
    pub height: Coord,
}

impl Default for Presentation {
    fn default() -> Self {
        Presentation {
            title: String::new(),
            author: String::new(),
            company: String::new(),
            revision: String::new(),
            subject: String::new(),
            rtl_mode: false,
            width: default_width(),
            height: default_height(),
        }
    }
}

fn default_width() -> Coord {
    Coord::Emu(12_196_763)
}
fn default_height() -> Coord {
    Coord::Emu(6_858_000)
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Theme {
    #[serde(default = "default_font")]
    pub major_font: String,
    #[serde(default = "default_font")]
    pub minor_font: String,
}

fn default_font() -> String {
    "Calibri".into()
}

fn default_theme() -> Theme {
    Theme {
        major_font: default_font(),
        minor_font: default_font(),
    }
}

/// The unified style tables: built-in defaults per shape type under
/// `[styles.<type>]` (`shape` is the fallback for every shape), and named
/// opt-in styles under `[styles.named.<name>]`.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Styles {
    /// `[styles.<type>]` — `shape`, `text`, `image`, `placeholder`, or a
    /// shape preset id.
    #[serde(flatten)]
    pub by_type: BTreeMap<String, Opts>,
    /// `[styles.named.<name>]` — reusable styles referenced by `style = "<name>"`.
    #[serde(default)]
    pub named: BTreeMap<String, Opts>,
}

/// The slide ordering index: each section pins `title` and the slide files.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Section {
    pub title: String,
    #[serde(default)]
    pub slides: Vec<String>,
}

/// `masters/<name>.toml`. The master name is the file stem; there is no
/// `title` field (the file name is the master name).
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Master {
    #[serde(default)]
    pub background: Option<toml::Value>,
    /// Margin in points/inches/EMU as a single number or `[t, r, b, l]`.
    #[serde(default)]
    pub margin: Option<toml::Value>,
    /// Positions a slide-number placeholder on every slide using this master.
    #[serde(default)]
    pub slide_number: Option<SlideNumber>,
    #[serde(default)]
    pub shapes: Vec<Shape>,
}

/// pptxgenjs `SlideNumberProps`: a positioned slide-number text box.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct SlideNumber {
    #[serde(default)]
    pub x: Option<Coord>,
    #[serde(default)]
    pub y: Option<Coord>,
    #[serde(default)]
    pub w: Option<Coord>,
    #[serde(default)]
    pub h: Option<Coord>,
    #[serde(flatten)]
    pub opts: Opts,
}

/// `slides/<name>.toml`.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slide {
    #[serde(default)]
    pub master: Option<String>,
    #[serde(default)]
    pub background: Option<toml::Value>,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub shapes: Vec<Shape>,
    #[serde(default)]
    pub notes: Option<String>,
}

/// A positioned element used by both slides and masters. `type` is `text`,
/// `image` or a pptxgenjs `ShapeType` preset (`rect`, `roundRect`, `line`,
/// ...). Reserved `chart`, `table` and `media` are recognised but not yet
/// implemented. In a master, `type = "placeholder"` defines a placeholder the
/// slide content can fill (`name` + `ph_type`: `title|body|pic|chart|tbl|media`).
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Shape {
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default)]
    pub style: Option<String>,
    /// Shorthand for a single paragraph of markdown text.
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub paragraphs: Vec<Para>,
    #[serde(default)]
    pub src: Option<String>,
    #[serde(default)]
    pub x: Option<Coord>,
    #[serde(default)]
    pub y: Option<Coord>,
    #[serde(default)]
    pub w: Option<Coord>,
    #[serde(default)]
    pub h: Option<Coord>,
    #[serde(flatten)]
    pub opts: Opts,
}

/// An explicit paragraph: a markdown string plus paragraph-level pptxgenjs
/// options (`bullet`, `align`, `level`, ...).
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Para {
    pub text: String,
    #[serde(flatten)]
    pub opts: Opts,
}

/// Which pptxgenjs call a shape maps to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    Shape,
    Image,
}

impl Shape {
    pub fn kind(&self) -> Result<Kind, String> {
        match self.ty.as_str() {
            "text" => Ok(Kind::Text),
            "image" => Ok(Kind::Image),
            "chart" | "table" | "media" => {
                Err(format!("shape type `{}` is not supported yet", self.ty))
            }
            _ => Ok(Kind::Shape),
        }
    }
}

impl Main {
    /// The built-in style defaults for a shape type: `[styles.shape]` (the
    /// fallback for every shape) layered with `[styles.text]` when the shape
    /// carries text, then the type-specific `[styles.<type>]` bucket. Later
    /// layers win, so `[styles.text]` is the fallback for text options and
    /// the type bucket overrides it. Bucket keys are matched by the type's
    /// canonical preset id (`roundRect`) or its snake form (`round_rect`).
    pub fn type_defaults_with_text(&self, ty: &str, carries_text: bool) -> toml::Table {
        let extend = |out: &mut toml::Table, key: &str| {
            if let Some(t) = self.styles.by_type.get(key) {
                for (k, v) in t {
                    out.insert(k.clone(), v.clone());
                }
            }
        };
        let mut out = toml::Table::new();
        extend(&mut out, "shape");
        if carries_text && ty != "text" {
            extend(&mut out, "text");
        }
        extend(&mut out, ty);
        if let Some(canon) = crate::opts::canonical_preset(ty) {
            extend(&mut out, canon);
        }
        out
    }
}

impl Project {
    /// Load `main.toml` from a deck directory.
    pub fn load(dir: impl Into<PathBuf>) -> Result<Self> {
        let dir = dir.into();
        let path = dir.join("main.toml");
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| miette!("cannot read `{}`: {e}", path.display()))?;
        let main =
            toml::from_str(&raw).map_err(|e| miette!("cannot parse `{}`: {e}", path.display()))?;
        Ok(Project { dir, main })
    }

    pub fn master_path(&self, stem: &str) -> PathBuf {
        self.dir.join("masters").join(format!("{stem}.toml"))
    }

    pub fn slide_path(&self, file: &str) -> PathBuf {
        self.dir.join("slides").join(file)
    }
}

impl Default for Theme {
    fn default() -> Self {
        default_theme()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn main_toml(s: &str) -> Main {
        toml::from_str(&format!("[presentation]\ntitle = \"deck\"\n{s}")).unwrap()
    }

    #[test]
    fn type_defaults_layer_shape_then_specific() {
        let main = main_toml(
            "[styles.shape]\nfill = { color = \"C7000A\" }\ncolor = \"FFFFFF\"\n[styles.rect]\nfill = { color = \"112233\" }\n",
        );
        // rect inherits the shared `color` but its own `fill` wins.
        let merged = main.type_defaults_with_text("rect", false);
        assert_eq!(merged.get("color").unwrap().as_str().unwrap(), "FFFFFF");
        assert!(
            merged
                .get("fill")
                .unwrap()
                .get("color")
                .unwrap()
                .as_str()
                .unwrap()
                == "112233"
        );
        // an unspecified preset falls back to [styles.shape] alone.
        let ellipse = main.type_defaults_with_text("ellipse", false);
        assert_eq!(ellipse.get("color").unwrap().as_str().unwrap(), "FFFFFF");
        assert!(ellipse.get("fill").is_some());
    }

    #[test]
    fn text_fallback_layers_beneath_type() {
        let main = main_toml(
            "[styles.text]\nfont_face = \"Arial\"\nfont_size = 12\n[styles.rect]\nfont_size = 14\n",
        );
        // A text-carrying rect gets the text font_face but its own font_size.
        let merged = main.type_defaults_with_text("rect", true);
        assert_eq!(merged.get("font_face").unwrap().as_str().unwrap(), "Arial");
        assert_eq!(merged.get("font_size").unwrap().as_integer().unwrap(), 14);
        // A plain rect (no text) does NOT pick up the [styles.text] fallback,
        // but still gets the [styles.rect] font_size.
        let plain = main.type_defaults_with_text("rect", false);
        assert!(plain.get("font_face").is_none());
        assert_eq!(plain.get("font_size").unwrap().as_integer().unwrap(), 14);
    }

    #[test]
    fn named_styles_deserialize() {
        let main = main_toml("[styles.named.muted]\nitalic = true\n");
        assert_eq!(main.styles.named.len(), 1);
        assert!(main.styles.named["muted"].get("italic").is_some());
    }
}
