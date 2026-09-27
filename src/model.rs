//! TOML model for a gwen deck, mirroring the pptxgenjs API surface.
//!
//! `main.toml` (`[presentation]`, `[theme]`, `[[sections]]`, `[defaults.<type>]`,
//! `[styles.<name>]`), `masters/<name>.toml` and `slides/<name>.toml`. Option
//! keys are snake_case and become the corresponding pptxgenjs camelCase
//! properties; arbitrary extra keys pass straight through.

use std::collections::BTreeMap;
use std::path::PathBuf;

use miette::{Result, miette};

use crate::opts;
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
    /// Named styles to apply, in order (later wins). Accepts a scalar
    /// (`styles = "muted"`) or a list (`styles = ["brand", "accent"]`).
    #[serde(default, deserialize_with = "one_or_many")]
    pub styles: Vec<String>,
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

/// Deserialise a field that is either a string or a list of strings
/// (`styles = "muted"` or `styles = ["brand", "accent"]`).
pub fn one_or_many<'de, D>(d: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct V;
    impl<'de> serde::de::Visitor<'de> for V {
        type Value = Vec<String>;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a string or a list of strings")
        }

        fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
            Ok(vec![v.to_string()])
        }

        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> Result<Self::Value, A::Error> {
            let mut out = Vec::new();
            while let Some(s) = seq.next_element::<String>()? {
                out.push(s);
            }
            Ok(out)
        }
    }
    d.deserialize_any(V)
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
    /// Layer a bucket's referenced `styles` (resolved recursively, in list
    /// order) and then its own inline keys, so the bucket's own options win
    /// over what it includes.
    fn apply_bucket(
        &self,
        out: &mut toml::Table,
        bucket: &toml::Table,
        path: &mut Vec<String>,
    ) -> Result<()> {
        if let Some(refs_value) = bucket.get("styles") {
            for name in opts::parse_styles_value(refs_value)
                .ok_or_else(|| miette!("`styles` must be a string or a list of strings"))?
            {
                self.resolve_named(out, &name, path)?;
            }
        }
        for (k, v) in bucket {
            if k != "styles" {
                out.insert(k.clone(), v.clone());
            }
        }
        Ok(())
    }

    /// A named style's effective options: its own `styles` references
    /// (depth-first) then its own inline keys. Cyclic references error.
    pub fn resolve_named(
        &self,
        out: &mut toml::Table,
        name: &str,
        path: &mut Vec<String>,
    ) -> Result<()> {
        if path.iter().any(|p| p == name) {
            return Err(miette!("styles cycle: {} -> {}", path.join(" -> "), name));
        }
        let bucket = self.styles.named.get(name).ok_or_else(|| {
            miette!("unknown style `{name}` (no `[styles.named.{name}]` in main.toml)")
        })?;
        path.push(name.to_string());
        self.apply_bucket(out, bucket, path)?;
        path.pop();
        Ok(())
    }

    /// The full style chain for a shape's options:
    /// `[styles.shape]` < `[styles.text]` (text-carrying) < `[styles.<type>]`
    /// < the shape's `styles` references; each bucket applies its own
    /// `styles` references before its inline keys. Later layers win.
    pub fn style_chain(
        &self,
        ty: &str,
        carries_text: bool,
        shape_styles: &[String],
    ) -> Result<toml::Table> {
        let mut out = toml::Table::new();
        let mut path: Vec<String> = Vec::new();
        if let Some(bucket) = self.styles.by_type.get("shape") {
            self.apply_bucket(&mut out, bucket, &mut path)?;
        }
        if carries_text
            && ty != "text"
            && let Some(text) = self.styles.by_type.get("text")
        {
            self.apply_bucket(&mut out, text, &mut path)?;
        }
        if let Some(bucket) = self.styles.by_type.get(ty) {
            self.apply_bucket(&mut out, bucket, &mut path)?;
        }
        if let Some(canon) = crate::opts::canonical_preset(ty)
            && canon != ty
            && let Some(bucket) = self.styles.by_type.get(canon)
        {
            self.apply_bucket(&mut out, bucket, &mut path)?;
        }
        for name in shape_styles {
            self.resolve_named(&mut out, name, &mut path)?;
        }
        Ok(out)
    }

    /// Validate every named-style reference and their cycle-freeness, so an
    /// unused broken style still fails at build time.
    pub fn validate_style_graph(&self) -> Result<()> {
        let roots: Vec<String> = self.styles.named.keys().cloned().collect();
        let mut path: Vec<String> = Vec::new();
        for root in &roots {
            let mut scratch = toml::Table::new();
            self.resolve_named(&mut scratch, root, &mut path)?;
        }
        Ok(())
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
        let merged = main.style_chain("rect", false, &[]).unwrap();
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
        let ellipse = main.style_chain("ellipse", false, &[]).unwrap();
        assert_eq!(ellipse.get("color").unwrap().as_str().unwrap(), "FFFFFF");
        assert!(ellipse.get("fill").is_some());
    }

    #[test]
    fn text_fallback_layers_beneath_type() {
        let main = main_toml(
            "[styles.text]\nfont_face = \"Arial\"\nfont_size = 12\n[styles.rect]\nfont_size = 14\n",
        );
        // A text-carrying rect gets the text font_face but its own font_size.
        let merged = main.style_chain("rect", true, &[]).unwrap();
        assert_eq!(merged.get("font_face").unwrap().as_str().unwrap(), "Arial");
        assert_eq!(merged.get("font_size").unwrap().as_integer().unwrap(), 14);
        // A plain rect (no text) does NOT pick up the [styles.text] fallback,
        // but still gets the [styles.rect] font_size.
        let plain = main.style_chain("rect", false, &[]).unwrap();
        assert!(plain.get("font_face").is_none());
        assert_eq!(plain.get("font_size").unwrap().as_integer().unwrap(), 14);
    }

    #[test]
    fn named_styles_compose_recursively() {
        let main = main_toml(
            "[styles.named.brand]\nfill = { color = \"C7000A\" }\n[styles.named.dark]\nstyles = [\"brand\"]\nfill = { color = \"111111\" }\n",
        );
        // dark inherits nothing else but its own fill wins over brand's.
        let merged = main.style_chain("rect", false, &["dark".into()]).unwrap();
        assert_eq!(
            merged
                .get("fill")
                .unwrap()
                .get("color")
                .unwrap()
                .as_str()
                .unwrap(),
            "111111"
        );
    }

    #[test]
    fn named_style_cycle_is_reported() {
        let main =
            main_toml("[styles.named.a]\nstyles = [\"b\"]\n[styles.named.b]\nstyles = [\"a\"]\n");
        let err = main.style_chain("rect", false, &["a".into()]).unwrap_err();
        assert!(err.to_string().contains("cycle"), "got: {err:?}");
    }

    #[test]
    fn unknown_named_style_is_reported() {
        let main = main_toml("[styles.named.a]\ncolor = \"111111\"\n");
        let err = main
            .style_chain("rect", false, &["nope".into()])
            .unwrap_err();
        assert!(
            err.to_string().contains("unknown style `nope`"),
            "got: {err:?}"
        );
    }

    #[test]
    fn named_styles_deserialize() {
        let main = main_toml("[styles.named.muted]\nitalic = true\n");
        assert_eq!(main.styles.named.len(), 1);
        assert!(main.styles.named["muted"].get("italic").is_some());
    }
}
