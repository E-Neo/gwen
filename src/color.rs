//! An RGBA color that every resolved scene value uses.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: f64,
}

impl Color {
    pub const BLACK: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 1.0,
    };
    pub const WHITE: Color = Color {
        r: 255,
        g: 255,
        b: 255,
        a: 1.0,
    };

    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Color { r, g, b, a: 1.0 }
    }

    /// Parse `#RGB`, `RGB`, or `RRGGBBAA` (CSS 8-digit, alpha included).
    /// Three-digit shorthand is expanded. `AARRGGBB` (OOXML's order) is not
    /// accepted: importers convert those to six digits before gwen sees them.
    pub fn hex(text: &str) -> Option<Self> {
        let s = text.trim().trim_start_matches('#');
        let parse = |part: &str| u8::from_str_radix(part, 16).ok();
        match s.len() {
            3 => {
                let one = |i: usize| parse(&s[i..i + 1]).map(|v| v * 17);
                Some(Color::new(one(0)?, one(1)?, one(2)?))
            }
            6 => Some(Color::new(
                parse(&s[0..2])?,
                parse(&s[2..4])?,
                parse(&s[4..6])?,
            )),
            8 => {
                let mut color = Color::new(parse(&s[0..2])?, parse(&s[2..4])?, parse(&s[4..6])?);
                color.a = parse(&s[6..8])? as f64 / 255.0;
                Some(color)
            }
            _ => None,
        }
    }

    pub fn with_alpha(self, a: f64) -> Self {
        Color {
            a: a.clamp(0.0, 1.0),
            ..self
        }
    }

    pub fn css(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    pub fn luminance(self) -> f64 {
        fn linear(v: u8) -> f64 {
            let c = v as f64 / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * linear(self.r) + 0.7152 * linear(self.g) + 0.0722 * linear(self.b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_forms() {
        assert_eq!(Color::hex("FF0000"), Some(Color::new(255, 0, 0)));
        assert_eq!(Color::hex("#f00"), Some(Color::new(255, 0, 0)));
        assert_eq!(
            Color::hex("00FF0080").map(|c| (c.r, c.g, c.b, c.a > 0.49 && c.a < 0.51)),
            Some((0, 255, 0, true))
        );
        assert_eq!(Color::hex("nope"), None);
    }

    #[test]
    fn alpha_clamps() {
        assert_eq!(Color::BLACK.with_alpha(2.0).a, 1.0);
        assert_eq!(Color::BLACK.with_alpha(-1.0).a, 0.0);
    }
}
