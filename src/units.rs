//! Length parsing: a coordinate is EMU by default, or a unit-suffixed string
//! (`"1in"`, `"2.5cm"`, `"25mm"`, `"72pt"`) or a percentage of the slide size
//! (`"50%"`).

/// EMU per inch.
pub const EMU_PER_IN: f64 = 914_400.0;

/// A coordinate value as written in TOML: an EMU integer or a unit string.
#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(untagged)]
pub enum Coord {
    Emu(i64),
    Text(String),
}

impl Coord {
    /// Resolve to EMU. Percentages are relative to `slide_emu` (width for x/w,
    /// height for y/h).
    pub fn emu(&self, slide_emu: i64) -> Result<i64, String> {
        match self {
            Coord::Emu(n) => Ok(*n),
            Coord::Text(s) => parse_emu(s, slide_emu),
        }
    }

    /// Resolve to inches (for the pptxgenjs spec).
    pub fn inches(&self, slide_emu: i64) -> Result<f64, String> {
        Ok(self.emu(slide_emu)? as f64 / EMU_PER_IN)
    }
}

fn parse_emu(input: &str, slide_emu: i64) -> Result<i64, String> {
    let s = input.trim();
    if let Some(pct) = s.strip_suffix('%') {
        let p: f64 = pct
            .trim()
            .parse()
            .map_err(|_| format!("invalid percentage `{input}`"))?;
        return Ok((slide_emu as f64 * p / 100.0).round() as i64);
    }
    for (unit, per_unit) in [
        ("in", EMU_PER_IN),
        ("cm", 360_000.0),
        ("mm", 36_000.0),
        ("pt", 12_700.0),
    ] {
        if let Some(num) = s.strip_suffix(unit)
            && !num.trim().is_empty()
        {
            let v: f64 = num
                .trim()
                .parse()
                .map_err(|_| format!("invalid length `{input}`"))?;
            return Ok((v * per_unit).round() as i64);
        }
    }
    s.parse::<i64>().map_err(|_| {
        format!("invalid length `{input}` (EMU, or `1in`/`2.5cm`/`25mm`/`72pt`/`50%`)")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_emu_and_units() {
        assert_eq!(Coord::Emu(914400).emu(9144000).unwrap(), 914400);
        assert_eq!(Coord::Text("1in".into()).emu(9144000).unwrap(), 914400);
        assert!(Coord::Text("25mmmmmm".into()).emu(9144000).is_err());
        assert_eq!(Coord::Text("1cm".into()).emu(9144000).unwrap(), 360000);
        assert_eq!(Coord::Text("72pt".into()).emu(9144000).unwrap(), 914400);
        assert_eq!(Coord::Text("50%".into()).emu(9144000).unwrap(), 4572000);
        assert!(Coord::Text("zzz".into()).emu(9144000).is_err());
    }

    #[test]
    fn converts_to_inches() {
        let c = Coord::Emu(12192000);
        assert!((c.inches(12192000).unwrap() - 13.33333).abs() < 0.001);
        assert_eq!(Coord::Text("1in".into()).inches(0).unwrap(), 1.0);
    }
}
