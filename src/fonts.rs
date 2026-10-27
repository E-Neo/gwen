//! Font discovery and real text metrics.
//!
//! Preview text is measured with the same font data resvg will render with:
//! deck fonts (`<deck>/fonts`), then `$GWEN_HOME/fonts` (default
//! `~/.gwen/fonts`), then the system. Widths come from harfrust shaping and
//! vertical metrics from skrifa, not a character-width guess.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use harfrust::{Buffer, ShapeOptions, ShaperFont};
use resvg::usvg::fontdb::{Database, FaceInfo, Family, ID, Query, Stretch, Style, Weight};
use skrifa::instance::{LocationRef, Size};
use skrifa::{FontRef, MetadataProvider};

const PT_PER_IN: f64 = 72.0;

/// A loaded font database plus per-face caches.
pub struct Fonts {
    db: Arc<Database>,
    cache: Mutex<HashMap<(ID, char), bool>>,
    face_fonts: Mutex<HashMap<ID, harfrust::Font>>,
    fallbacks: Vec<ID>,
    default_family: String,
    /// A CJK family that is actually installed, used by the preview when the
    /// requested family is missing (e.g. `Microsoft YaHei` on Linux).
    cjk_family: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct Metrics {
    /// Ascender, positive, in the requested size units.
    pub ascent: f64,
    /// Descender, positive (the absolute value of the font's negative one).
    pub descent: f64,
    pub line_gap: f64,
}

impl Metrics {
    pub fn line_height(&self) -> f64 {
        self.ascent + self.descent + self.line_gap
    }
}

impl Fonts {
    /// Load the deck's fonts, the gwen home fonts, then system fonts.
    pub fn load(deck: &Path) -> Fonts {
        let mut db = Database::new();
        load_dir(&mut db, &deck.join("fonts"));
        if let Some(home) = gwen_home() {
            load_dir(&mut db, &home.join("fonts"));
        }
        db.load_system_fonts();
        // `fontdb` stores family names; the generic aliases are just strings.
        // Pick names that really exist, otherwise usvg's fallback (which always
        // ends in `Family::Serif`) resolves to nothing and text disappears.
        let sans = available_family(
            &db,
            &[
                "DejaVu Sans",
                "Noto Sans",
                "Liberation Sans",
                "Arial",
                "Helvetica",
                "Adwaita Sans",
            ],
        )
        .or_else(|| first_family(&db, |face| !face.monospaced))
        .unwrap_or_else(|| "sans-serif".to_string());
        let serif = available_family(
            &db,
            &[
                "DejaVu Serif",
                "Noto Serif",
                "Liberation Serif",
                "Times New Roman",
            ],
        )
        .unwrap_or_else(|| sans.clone());
        let mono = available_family(
            &db,
            &[
                "DejaVu Sans Mono",
                "Noto Sans Mono",
                "Liberation Mono",
                "Courier New",
                "Adwaita Mono",
            ],
        )
        .or_else(|| first_family(&db, |face| face.monospaced))
        .unwrap_or_else(|| "monospace".to_string());
        db.set_sans_serif_family(&sans);
        db.set_serif_family(&serif);
        db.set_monospace_family(&mono);
        let db = Arc::new(db);
        let cjk_family = available_family(
            &db,
            &[
                "Noto Sans CJK SC",
                "Noto Sans CJK JP",
                "Noto Sans CJK TC",
                "Noto Sans SC",
                "Noto Sans JP",
                "Source Han Sans SC",
            ],
        );
        let fallbacks = [
            "Noto Sans CJK SC",
            "Noto Sans CJK JP",
            "Noto Sans",
            "DejaVu Sans",
            "Arial Unicode MS",
            "Segoe UI Symbol",
        ]
        .into_iter()
        .filter_map(|name| {
            db.query(&Query {
                families: &[Family::Name(name)],
                weight: Weight::NORMAL,
                stretch: Stretch::Normal,
                style: Style::Normal,
            })
        })
        .collect();
        Fonts {
            db,
            cache: Mutex::new(HashMap::new()),
            face_fonts: Mutex::new(HashMap::new()),
            fallbacks,
            default_family: sans,
            cjk_family,
        }
    }

    /// Whether a family is installed. Unlike [`Fonts::face`] this does not
    /// fall back to the default family.
    fn has_family(&self, family: &str) -> bool {
        self.db
            .query(&Query {
                families: &[Family::Name(family)],
                weight: Weight::NORMAL,
                stretch: Stretch::Normal,
                style: Style::Normal,
            })
            .is_some()
    }

    /// The family used to paint preview text. A missing family falls back to
    /// the installed CJK family for CJK runs (so bold and italic keep a real
    /// face) and to the generic sans family for Latin-only runs (so arrows and
    /// digits keep their normal baseline).
    pub fn preview_family(&self, family: &str, text: &str) -> String {
        if self.has_family(family) {
            return family.to_string();
        }
        if text.chars().any(is_cjk)
            && let Some(cjk) = &self.cjk_family
        {
            return cjk.clone();
        }
        self.default_family.clone()
    }

    /// The font database, shared with usvg so rendering and measuring select
    /// the same faces.
    pub fn database(&self) -> Arc<Database> {
        Arc::clone(&self.db)
    }

    /// Resolve a family name (with bold/italic) to a face.
    pub fn face(&self, family: &str, bold: bool, italic: bool) -> Option<ID> {
        let weight = if bold { Weight::BOLD } else { Weight::NORMAL };
        let style = if italic { Style::Italic } else { Style::Normal };
        self.db
            .query(&Query {
                families: &[Family::Name(family)],
                weight,
                stretch: Stretch::Normal,
                style,
            })
            .or_else(|| {
                self.db.query(&Query {
                    families: &[Family::Name(&self.default_family)],
                    weight,
                    stretch: Stretch::Normal,
                    style,
                })
            })
    }

    fn face_for_char(&self, primary: ID, ch: char) -> ID {
        if self.covers(primary, ch) {
            return primary;
        }
        for id in &self.fallbacks {
            if self.covers(*id, ch) {
                return *id;
            }
        }
        for face in self.db.faces() {
            if self.covers(face.id, ch) {
                return face.id;
            }
        }
        primary
    }

    fn covers(&self, id: ID, ch: char) -> bool {
        let key = (id, ch);
        if let Some(value) = self
            .cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&key)
        {
            return *value;
        }
        let covers = self
            .db
            .with_face_data(id, |data, index| {
                FontRef::from_index(data, index)
                    .ok()
                    .and_then(|font| font.charmap().map(ch))
                    .is_some()
            })
            .unwrap_or(false);
        self.cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key, covers);
        covers
    }

    fn face_font(&self, id: ID) -> Option<harfrust::Font> {
        if let Some(font) = self
            .face_fonts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .cloned()
        {
            return Some(font);
        }
        let font = self
            .db
            .with_face_data(id, |data, index| harfrust::Font::new(data.to_vec(), index))
            .flatten()?;
        self.face_fonts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, font.clone());
        Some(font)
    }

    /// Vertical metrics for a face at `size_pt`.
    pub fn metrics(&self, id: ID, size_pt: f64) -> Metrics {
        let metrics = self.db.with_face_data(id, |data, index| {
            let font = FontRef::from_index(data, index).ok()?;
            let metrics = font.metrics(Size::unscaled(), LocationRef::new(&[]));
            let upem = metrics.units_per_em.max(1) as f64;
            let scale = size_pt / upem;
            Some(Metrics {
                ascent: metrics.ascent as f64 * scale,
                descent: (-metrics.descent as f64).max(0.0) * scale,
                line_gap: metrics.leading.max(0.0) as f64 * scale,
            })
        });
        metrics.flatten().unwrap_or(Metrics {
            ascent: size_pt * 0.8,
            descent: size_pt * 0.2,
            line_gap: 0.0,
        })
    }

    /// Metrics for a family, falling back to the sans-serif face.
    pub fn family_metrics(&self, family: &str, bold: bool, size_pt: f64) -> Metrics {
        match self.face(family, bold, false) {
            Some(id) => self.metrics(id, size_pt),
            None => Metrics {
                ascent: size_pt * 0.8,
                descent: size_pt * 0.2,
                line_gap: 0.0,
            },
        }
    }

    /// Shaped advance width of `text` in points, with per-character fallback.
    pub fn measure(&self, text: &str, family: &str, bold: bool, italic: bool, size_pt: f64) -> f64 {
        if text.is_empty() {
            return 0.0;
        }
        let primary = match self.face(family, bold, italic) {
            Some(id) => id,
            None => return self.heuristic(text, size_pt),
        };
        let mut total = 0.0;
        let mut segment = String::new();
        let mut segment_font = primary;
        for ch in text.chars() {
            let font = self.face_for_char(primary, ch);
            if font != segment_font && !segment.is_empty() {
                total += self.shape_segment(&segment, segment_font, size_pt);
                segment.clear();
            }
            segment_font = font;
            segment.push(ch);
        }
        if !segment.is_empty() {
            total += self.shape_segment(&segment, segment_font, size_pt);
        }
        total
    }

    /// Width of one run including `char_spacing` in points.
    pub fn measure_spaced(
        &self,
        text: &str,
        family: &str,
        bold: bool,
        italic: bool,
        size_pt: f64,
        char_spacing_pt: f64,
    ) -> f64 {
        let base = self.measure(text, family, bold, italic, size_pt);
        base + char_spacing_pt * text.chars().count() as f64
    }

    fn shape_segment(&self, text: &str, id: ID, size_pt: f64) -> f64 {
        let shaped = self.face_font(id).and_then(|font| {
            let upem = font.units_per_em().max(1) as f64;
            let shaper = ShaperFont::new(&font);
            let mut buffer = Buffer::new();
            buffer.push_str(text);
            buffer.guess_segment_properties();
            harfrust::shape(&shaper, &mut buffer, ShapeOptions::new()).ok()?;
            let advance: i32 = buffer
                .glyph_positions()
                .iter()
                .map(|position| position.x_advance)
                .sum();
            Some(advance as f64 * size_pt / upem)
        });
        shaped.unwrap_or_else(|| self.heuristic(text, size_pt))
    }

    fn heuristic(&self, text: &str, size_pt: f64) -> f64 {
        text.chars()
            .map(|c| {
                if c == ' ' {
                    0.28
                } else if is_cjk(c) {
                    1.0
                } else {
                    0.52
                }
            })
            .sum::<f64>()
            * size_pt
    }

    /// Points to inches.
    pub fn pt_to_in(pt: f64) -> f64 {
        pt / PT_PER_IN
    }
}

fn load_dir(db: &mut Database, dir: &Path) {
    if dir.is_dir() {
        db.load_fonts_dir(dir);
    }
}

fn available_family(db: &Database, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        db.faces()
            .any(|face| {
                face.families
                    .iter()
                    .any(|(family, _)| family.eq_ignore_ascii_case(name))
            })
            .then(|| (*name).to_string())
    })
}

fn first_family(db: &Database, pick: impl Fn(&FaceInfo) -> bool) -> Option<String> {
    db.faces()
        .find(|face| pick(face))
        .and_then(|face| face.families.first().map(|(name, _)| name.clone()))
}

fn gwen_home() -> Option<PathBuf> {
    if let Ok(home) = std::env::var("GWEN_HOME")
        && !home.trim().is_empty()
    {
        return Some(PathBuf::from(home));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".gwen"))
}

fn is_cjk(c: char) -> bool {
    matches!(
        c as u32,
        0x2E80..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_and_advances_are_finite() {
        let dir = std::env::temp_dir().join("gwen-fonts-test");
        let fonts = Fonts::load(&dir);
        let family = fonts
            .db
            .faces()
            .find_map(|f| f.families.first().map(|(name, _)| name.clone()));
        let Some(family) = family else { return };
        let m = fonts.family_metrics(&family, false, 24.0);
        assert!(m.ascent > 0.0, "ascent {m:?}");
        assert!(m.line_height() > 0.0);
        let w = fonts.measure("Hello", &family, false, false, 24.0);
        assert!(w > 0.0, "advance {w}");
        assert_eq!(fonts.measure("", &family, false, false, 24.0), 0.0);
    }

    #[test]
    fn preview_family_falls_back_to_an_installed_face() {
        let dir = std::env::temp_dir().join("gwen-fonts-preview-test");
        let fonts = Fonts::load(&dir);
        for text in ["plain", "中文"] {
            let family = fonts.preview_family("__gwen_missing_family__", text);
            assert!(
                fonts.has_family(&family),
                "preview fallback `{family}` is not installed"
            );
        }
    }
}
