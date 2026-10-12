//! Media helpers: MIME lookup and the intrinsic aspect ratio of an image
//! source, so a shape can give only `w` or only `h` and gwen derives the
//! missing dimension from the image itself (PNG/JPEG/GIF/BMP headers, SVG
//! `viewBox`/`width`/`height`).

use std::path::Path;

/// Map a media file extension to its MIME type.
pub fn mime_for(filename: &str) -> Result<&'static str, String> {
    let ext = filename.rsplit('.').next().unwrap_or("");
    match ext.to_ascii_lowercase().as_str() {
        "png" => Ok("image/png"),
        "jpg" | "jpeg" => Ok("image/jpeg"),
        "gif" => Ok("image/gif"),
        "bmp" => Ok("image/bmp"),
        "tif" | "tiff" => Ok("image/tiff"),
        "svg" => Ok("image/svg+xml"),
        other => Err(format!("unsupported image extension `.{other}`")),
    }
}

/// Width/height ratio (`w / h`) of an image file, read from its headers.
pub fn image_aspect(path: &Path) -> Result<f64, String> {
    let bytes =
        std::fs::read(path).map_err(|e| format!("cannot read `{}`: {e}", path.display()))?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let ratio = match ext.as_str() {
        "png" => png_aspect(&bytes),
        "jpg" | "jpeg" => jpeg_aspect(&bytes),
        "gif" => gif_aspect(&bytes),
        "bmp" => bmp_aspect(&bytes),
        "svg" => std::str::from_utf8(&bytes).ok().and_then(svg_aspect),
        _ => None,
    };
    ratio.ok_or_else(|| format!("cannot determine the dimensions of `{}`", path.display()))
}

fn ratio(w: u32, h: u32) -> Option<f64> {
    if w == 0 || h == 0 {
        None
    } else {
        Some(w as f64 / h as f64)
    }
}

fn png_aspect(b: &[u8]) -> Option<f64> {
    if b.len() >= 24
        && b[0..8] == [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]
        && &b[12..16] == b"IHDR"
    {
        let w = u32::from_be_bytes(b[16..20].try_into().ok()?);
        let h = u32::from_be_bytes(b[20..24].try_into().ok()?);
        ratio(w, h)
    } else {
        None
    }
}

fn jpeg_aspect(b: &[u8]) -> Option<f64> {
    let mut i = 2;
    while i + 9 <= b.len() {
        if b[i] != 0xff {
            i += 1;
            continue;
        }
        let marker = b[i + 1];
        if marker == 0xd8 || marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            i += 2;
            continue;
        }
        if marker == 0xd9 || marker == 0xda {
            break;
        }
        if i + 4 > b.len() {
            return None;
        }
        let len = (b[i + 2] as usize) << 8 | b[i + 3] as usize;
        // SOF0..SOF15 except DHT(0xc4), JPG(0xc8), DAC(0xcc).
        if (0xc0..=0xcf).contains(&marker)
            && marker != 0xc4
            && marker != 0xc8
            && marker != 0xcc
            && i + 9 <= b.len()
        {
            let h = (b[i + 5] as u32) << 8 | b[i + 6] as u32;
            let w = (b[i + 7] as u32) << 8 | b[i + 8] as u32;
            return ratio(w, h);
        }
        if len < 2 {
            return None;
        }
        i += 2 + len;
    }
    None
}

fn gif_aspect(b: &[u8]) -> Option<f64> {
    if b.len() >= 10 && (&b[0..6] == b"GIF87a" || &b[0..6] == b"GIF89a") {
        let w = u16::from_le_bytes(b[6..8].try_into().ok()?) as u32;
        let h = u16::from_le_bytes(b[8..10].try_into().ok()?) as u32;
        ratio(w, h)
    } else {
        None
    }
}

fn bmp_aspect(b: &[u8]) -> Option<f64> {
    if b.len() >= 26 && &b[0..2] == b"BM" {
        let w = i32::from_le_bytes(b[18..22].try_into().ok()?);
        let h = i32::from_le_bytes(b[22..26].try_into().ok()?);
        if w <= 0 || h == 0 {
            return None;
        }
        ratio(w as u32, h.unsigned_abs())
    } else {
        None
    }
}

/// SVG aspect from the root `<svg>` element: prefer `viewBox="minX minY w h"`
/// (last two numbers), else `width`/`height` attributes with unit suffixes
/// stripped when both parse.
fn svg_aspect(text: &str) -> Option<f64> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_reader(text.as_bytes());
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let qname = e.name();
                let name = qname.as_ref();
                let local = name.rfind(':').map_or(name, |p| &name[p + 1..]);
                if local != "svg" {
                    buf.clear();
                    continue;
                }
                let mut width = None;
                let mut height = None;
                let mut viewbox = None;
                for attr in e.attributes() {
                    let Ok(attr) = attr else { continue };
                    let key = attr.key.as_ref();
                    let key = key.rfind(':').map_or(key, |p| &key[p + 1..]);
                    let value = attr
                        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        .ok()?;
                    match key {
                        "viewBox" => viewbox = parse_viewbox(&value),
                        "width" => width = parse_number(&value),
                        "height" => height = parse_number(&value),
                        _ => {}
                    }
                }
                if let Some((w, h)) = viewbox {
                    return ratio_f(w, h);
                }
                if let (Some(w), Some(h)) = (width, height) {
                    return ratio_f(w, h);
                }
                return None;
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => return None,
        }
        buf.clear();
    }
    None
}

fn parse_viewbox(value: &str) -> Option<(f64, f64)> {
    let nums: Vec<f64> = value
        .split_whitespace()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .filter_map(|t| t.parse().ok())
        .collect();
    let (w, h) = (
        nums.get(nums.len().saturating_sub(2))?.to_owned(),
        nums.last()?.to_owned(),
    );
    Some((w, h))
}

/// Parse a length value, ignoring any unit suffix (`"100px"`, `"50%"`, `"2in"`).
fn parse_number(value: &str) -> Option<f64> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let digits: String = trimmed
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == '+')
        .collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

fn ratio_f(w: f64, h: f64) -> Option<f64> {
    if w <= 0.0 || h <= 0.0 {
        None
    } else {
        Some(w / h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_header() {
        let mut b = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
        b.extend_from_slice(&[0, 0, 0, 13, b'I', b'H', b'D', b'R']);
        b.extend_from_slice(&200u32.to_be_bytes());
        b.extend_from_slice(&100u32.to_be_bytes());
        assert_eq!(png_aspect(&b), Some(2.0));
        assert_eq!(png_aspect(b"nope"), None);
    }

    #[test]
    fn svg_viewbox() {
        assert_eq!(
            svg_aspect(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 100"></svg>"#),
            Some(2.0)
        );
    }

    #[test]
    fn svg_width_height_units() {
        assert_eq!(
            svg_aspect(r#"<svg width="100px" height="50%" />"#),
            Some(2.0)
        );
        assert_eq!(svg_aspect("<svg width=\"100px\"/>"), None);
    }
}
