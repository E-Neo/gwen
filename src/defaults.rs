//! The generator-side defaults gwen must reproduce to preview a deck.
//!
//! The theme palette, `+mj-lt`/`+mn-lt` font roles and the shared master's
//! `titleStyle` / `bodyStyle` / `otherStyle` are the same static parts the
//! native PPTX writer emits.  Parse them once rather than duplicating the
//! tables in Rust.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};

use miette::{Result, miette};

use crate::color::Color;
use crate::xml::{self, Node};

const EMU_PER_IN: f64 = 914_400.0;

/// One `a:lvlNpPr` from the master's text styles.
#[derive(Debug, Clone, Default)]
pub struct LevelStyle {
    /// Point size (`sz` is hundredths of a point).
    pub size_pt: Option<f64>,
    pub bold: bool,
    pub italic: bool,
    pub color: Option<Color>,
    pub font: Option<String>,
    pub align: Option<String>,
    pub mar_l: f64,
    pub indent: f64,
    pub bullet: Option<BulletDefault>,
    pub line_spacing_pct: Option<f64>,
    pub space_before_pt: f64,
    pub space_after_pt: f64,
}

#[derive(Debug, Clone)]
pub enum BulletDefault {
    Char(String),
    Number(String),
    None,
}

#[derive(Debug, Clone)]
pub struct Defaults {
    /// `tx1`, `bg1`, `tx2`, `bg2`, `accent1..6`, `hlink`, `folHlink`.
    pub palette: BTreeMap<String, Color>,
    pub major_font: String,
    pub minor_font: String,
    pub title: LevelStyle,
    /// Levels 1..9 (index 0 is level 1).
    pub body: Vec<LevelStyle>,
    pub other: LevelStyle,
}

impl Defaults {
    /// Resolve a theme color name (`tx1`, `accent2`, ...) to its palette entry.
    pub fn theme_color(&self, name: &str) -> Option<Color> {
        self.palette.get(name).copied()
    }

    /// Resolve a font role: `+mj-lt`/`+mj-ea` -> major, `+mn-lt`/... -> minor.
    pub fn font_role(&self, name: &str) -> Option<&str> {
        let name = name.trim();
        if let Some(rest) = name.strip_prefix("+mj") {
            let _ = rest;
            Some(&self.major_font)
        } else if let Some(rest) = name.strip_prefix("+mn") {
            let _ = rest;
            Some(&self.minor_font)
        } else {
            None
        }
    }

    /// The style a placeholder level inherits when the shape sets nothing.
    pub fn placeholder_style(&self, ph_type: &str, level: usize) -> &LevelStyle {
        match ph_type {
            "title" | "ctrTitle" => &self.title,
            "body" | "obj" | "subTitle" => self
                .body
                .get(level.saturating_sub(1))
                .or_else(|| self.body.first())
                .unwrap_or(&self.other),
            _ => &self.other,
        }
    }
}

type FontKey = (String, String);
type CachedDefaults = Option<(FontKey, Arc<Defaults>)>;

static CACHE: OnceLock<Mutex<CachedDefaults>> = OnceLock::new();

/// pptxgenjs's defaults for these theme fonts, parsed once per process.
pub fn get(major_font: &str, minor_font: &str) -> Result<Arc<Defaults>> {
    let key = (major_font.to_string(), minor_font.to_string());
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((cached_key, value)) = guard.as_ref()
        && cached_key == &key
    {
        return Ok(value.clone());
    }
    let theme = crate::pptx::default_theme_xml(major_font, minor_font);
    let master = crate::pptx::default_master_styles_xml();
    let value = Arc::new(parse(&theme, &master)?);
    *guard = Some((key, value.clone()));
    Ok(value)
}

fn parse(theme: &str, master: &str) -> Result<Defaults> {
    let theme = xml::parse(theme).map_err(|e| miette!("theme1.xml: {e}"))?;
    let master = xml::parse(master).map_err(|e| miette!("slideMaster1.xml: {e}"))?;

    let mut palette = BTreeMap::new();
    if let Some(scheme) = theme.find("clrScheme") {
        for (slot, name) in [
            ("dk1", "tx1"),
            ("lt1", "bg1"),
            ("dk2", "tx2"),
            ("lt2", "bg2"),
            ("accent1", "accent1"),
            ("accent2", "accent2"),
            ("accent3", "accent3"),
            ("accent4", "accent4"),
            ("accent5", "accent5"),
            ("accent6", "accent6"),
            ("hlink", "hlink"),
            ("folHlink", "folHlink"),
        ] {
            if let Some(color) = scheme.child(slot).and_then(scheme_color) {
                palette.insert(name.to_string(), color);
            }
        }
    }

    let major_font = theme
        .find("majorFont")
        .and_then(|f| f.child("latin"))
        .and_then(|l| l.attr("typeface"))
        .unwrap_or("Calibri")
        .to_string();
    let minor_font = theme
        .find("minorFont")
        .and_then(|f| f.child("latin"))
        .and_then(|l| l.attr("typeface"))
        .unwrap_or("Calibri")
        .to_string();

    let empty = LevelStyle::default();
    let title = style_at(&master, "titleStyle", 1, &palette, &major_font).unwrap_or(empty.clone());
    let other = style_at(&master, "otherStyle", 1, &palette, &major_font).unwrap_or(empty.clone());
    let mut body = Vec::new();
    for level in 1..=9 {
        match style_at(&master, "bodyStyle", level, &palette, &minor_font) {
            Some(style) => body.push(style),
            None => body.push(other.clone()),
        }
    }

    Ok(Defaults {
        palette,
        major_font,
        minor_font,
        title,
        body,
        other,
    })
}

fn style_at(
    master: &Node,
    bucket: &str,
    level: usize,
    palette: &BTreeMap<String, Color>,
    fallback_font: &str,
) -> Option<LevelStyle> {
    let style = master.find(bucket)?;
    let lvl = style.child(&format!("lvl{level}pPr"))?;
    let def = lvl.child("defRPr");
    let mut out = LevelStyle {
        mar_l: lvl.attr_f64("marL").unwrap_or(0.0) / EMU_PER_IN,
        indent: lvl.attr_f64("indent").unwrap_or(0.0) / EMU_PER_IN,
        align: lvl.attr("algn").map(String::from),
        ..LevelStyle::default()
    };
    if let Some(def) = def {
        out.size_pt = def.attr_f64("sz").map(|v| v / 100.0);
        out.bold = truthy(def.attr("b"));
        out.italic = truthy(def.attr("i"));
        out.color = def
            .child("solidFill")
            .and_then(|fill| fill_color(fill, palette));
        out.font = def
            .child("latin")
            .and_then(|latin| latin.attr("typeface"))
            .map(|name| {
                if name.starts_with("+mj") || name.starts_with("+mn") {
                    fallback_font.to_string()
                } else {
                    name.to_string()
                }
            });
    }
    if let Some(char) = lvl.find("buChar").and_then(|b| b.attr("char")) {
        out.bullet = Some(BulletDefault::Char(char.to_string()));
    } else if let Some(kind) = lvl.find("buAutoNum").and_then(|b| b.attr("type")) {
        out.bullet = Some(BulletDefault::Number(kind.to_string()));
    } else if lvl.find("buNone").is_some() {
        out.bullet = Some(BulletDefault::None);
    }
    if let Some(pct) = lvl
        .child("lnSpc")
        .and_then(|n| n.child("spcPct"))
        .and_then(|n| n.attr_f64("val"))
    {
        out.line_spacing_pct = Some(pct / 100_000.0);
    }
    out.space_before_pt = lvl
        .child("spcBef")
        .and_then(|n| n.child("spcPts"))
        .and_then(|n| n.attr_f64("val"))
        .map(|v| v / 100.0)
        .unwrap_or(0.0);
    out.space_after_pt = lvl
        .child("spcAft")
        .and_then(|n| n.child("spcPts"))
        .and_then(|n| n.attr_f64("val"))
        .map(|v| v / 100.0)
        .unwrap_or(0.0);
    Some(out)
}

fn scheme_color(node: &Node) -> Option<Color> {
    if let Some(srgb) = node.child("srgbClr") {
        return srgb.attr("val").and_then(Color::hex);
    }
    node.child("sysClr")
        .and_then(|sys| sys.attr("lastClr"))
        .and_then(Color::hex)
}

fn fill_color(fill: &Node, palette: &BTreeMap<String, Color>) -> Option<Color> {
    if let Some(srgb) = fill.child("srgbClr") {
        return srgb.attr("val").and_then(Color::hex);
    }
    if let Some(scheme) = fill.child("schemeClr") {
        let name = scheme.attr("val")?;
        return palette.get(name).copied().or(match name {
            "tx1" => Some(Color::BLACK),
            "bg1" => Some(Color::WHITE),
            _ => None,
        });
    }
    fill.child("sysClr")
        .and_then(|sys| sys.attr("lastClr"))
        .and_then(Color::hex)
}

fn truthy(value: Option<&str>) -> bool {
    matches!(value, Some("1") | Some("true") | Some("on"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_native_default_theme_and_master() {
        let d = get("Calibri", "Calibri").unwrap();
        assert_eq!(d.title.size_pt, Some(44.0), "pptxgenjs title default");
        assert!(d.palette.contains_key("accent1"), "theme palette parsed");
        assert!(d.palette.contains_key("tx1"));
        assert!(!d.body.is_empty(), "body styles parsed");
        assert_eq!(d.body[0].size_pt, Some(32.0), "body level 1 default");
        assert_eq!(d.other.size_pt, Some(18.0), "other default");
        assert_eq!(d.major_font, "Calibri");
    }

    #[test]
    fn parses_a_minimal_theme_and_master() {
        let theme = r#"<a:theme xmlns:a="a"><a:themeElements><a:clrScheme>
            <a:dk1><a:sysClr lastClr="000000"/></a:dk1>
            <a:lt1><a:sysClr lastClr="FFFFFF"/></a:lt1>
            <a:accent1><a:srgbClr val="4472C4"/></a:accent1>
            </a:clrScheme><a:fontScheme>
            <a:majorFont><a:latin typeface="Georgia"/></a:majorFont>
            <a:minorFont><a:latin typeface="Verdana"/></a:minorFont>
            </a:fontScheme></a:themeElements></a:theme>"#;
        let master = r#"<p:sldMaster xmlns:p="p" xmlns:a="a"><p:txStyles>
            <p:titleStyle><a:lvl1pPr algn="ctr"><a:defRPr sz="4400" b="1"><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill><a:latin typeface="+mj-lt"/></a:defRPr></a:lvl1pPr></p:titleStyle>
            <p:bodyStyle><a:lvl1pPr marL="342900" indent="-342900"><a:defRPr sz="3200"><a:latin typeface="+mn-lt"/></a:defRPr><a:buChar char="&#8226;"/></a:lvl1pPr></p:bodyStyle>
            <p:otherStyle><a:lvl1pPr><a:defRPr sz="1800"/></a:lvl1pPr></p:otherStyle>
            </p:txStyles></p:sldMaster>"#;
        let d = parse(theme, master).unwrap();
        assert_eq!(d.theme_color("accent1"), Some(Color::new(0x44, 0x72, 0xC4)));
        assert_eq!(d.major_font, "Georgia");
        assert_eq!(d.title.size_pt, Some(44.0));
        assert!(d.title.bold);
        assert_eq!(d.title.color, Some(Color::new(255, 0, 0)));
        assert_eq!(d.title.align.as_deref(), Some("ctr"));
        assert_eq!(d.title.font.as_deref(), Some("Georgia"));
        assert_eq!(d.body[0].size_pt, Some(32.0));
        assert_eq!(d.body[0].mar_l, 342900.0 / EMU_PER_IN);
        assert!(matches!(d.body[0].bullet, Some(BulletDefault::Char(ref c)) if c == "\u{2022}"));
        assert_eq!(d.placeholder_style("body", 2).size_pt, Some(18.0));
        assert_eq!(d.font_role("+mn-lt"), Some("Verdana"));
    }
}
