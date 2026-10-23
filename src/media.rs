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

/// Rasterize an SVG to PNG bytes with `resvg` (pure Rust, offline). `target`
/// is a desired pixel box; the SVG scales to fit inside it preserving aspect.
/// Without a target the SVG renders at its natural size. The PNG keeps a
/// transparent background so SVG transparency survives.
pub fn svg_to_png(bytes: &[u8], target: Option<(f64, f64)>) -> Result<Vec<u8>, String> {
    let mut opt = resvg::usvg::Options::default();
    opt.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_data(bytes, &opt).map_err(|e| e.to_string())?;
    let size = tree.size();
    let (sw, sh) = (f64::from(size.width()), f64::from(size.height()));
    if sw <= 0.0 || sh <= 0.0 {
        return Err("the SVG has no usable size".into());
    }
    // Fit the target box (or natural size), then clamp the output dimensions
    // to [1, 16384] so a pathological SVG can't allocate a giant pixmap.
    let max_dim = sw.max(sh);
    let scale = target.map_or(1.0, |(tw, th)| (tw / sw).min(th / sh));
    let scale = scale.clamp(1.0 / max_dim, 16384.0 / max_dim);
    let ow = (sw * scale).round().clamp(1.0, 16384.0) as u32;
    let oh = (sh * scale).round().clamp(1.0, 16384.0) as u32;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(ow, oh)
        .ok_or_else(|| format!("cannot allocate a {ow}x{oh} pixmap"))?;
    let transform =
        resvg::tiny_skia::Transform::from_scale(ow as f32 / sw as f32, oh as f32 / sh as f32);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    pixmap.encode_png().map_err(|e| e.to_string())
}

/// Make an image data URI safe for `usvg`'s decoder. PNG/JPEG/GIF/SVG pass
/// through; BMP and TIFF are transcoded to PNG because usvg does not decode
/// those formats.
pub fn preview_data_uri(uri: &str) -> Result<String, String> {
    let rest = uri
        .strip_prefix("data:")
        .ok_or_else(|| "image data is not a data URI".to_string())?;
    let (meta, payload) = rest
        .split_once(',')
        .ok_or_else(|| "image data URI has no comma".to_string())?;
    let mime = meta.split(';').next().unwrap_or("");
    if !matches!(
        mime,
        "image/bmp" | "image/x-ms-bmp" | "image/tiff" | "image/tif"
    ) {
        return Ok(uri.to_string());
    }
    let bytes = if meta.ends_with(";base64") {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(payload)
            .map_err(|e| format!("invalid base64 in image data: {e}"))?
    } else {
        payload.as_bytes().to_vec()
    };
    let rgba = if mime.contains("bmp") {
        decode_bmp(&bytes)?
    } else {
        decode_tiff(&bytes)?
    };
    let png = encode_png(rgba.0, rgba.1, &rgba.2)?;
    use base64::Engine;
    Ok(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    ))
}

/// Decode an uncompressed or BI_BITFIELDS BMP (8/24/32 bits per pixel) to
/// straight RGBA.
fn decode_bmp(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    if bytes.len() < 54 || &bytes[0..2] != b"BM" {
        return Err("not a BMP file".into());
    }
    let offset = u32::from_le_bytes(bytes[10..14].try_into().unwrap()) as usize;
    let header = u32::from_le_bytes(bytes[14..18].try_into().unwrap()) as usize;
    if header < 40 {
        return Err("unsupported BMP header".into());
    }
    let width = i32::from_le_bytes(bytes[18..22].try_into().unwrap());
    let raw_height = i32::from_le_bytes(bytes[22..26].try_into().unwrap());
    let bpp = u16::from_le_bytes(bytes[28..30].try_into().unwrap());
    let compression = u32::from_le_bytes(bytes[30..34].try_into().unwrap());
    if width <= 0 || raw_height == 0 {
        return Err("BMP has no pixels".into());
    }
    let top_down = raw_height < 0;
    let height = raw_height.unsigned_abs();
    if !matches!(bpp, 8 | 24 | 32) || !matches!(compression, 0 | 3) {
        return Err(format!(
            "unsupported BMP format ({bpp} bpp, compression {compression})"
        ));
    }
    let width = width as usize;
    let height = height as usize;
    let stride = (width * bpp as usize).div_ceil(32) * 4;
    let need = offset
        .checked_add(stride.saturating_mul(height))
        .ok_or("BMP is too large")?;
    if need > bytes.len() {
        return Err("truncated BMP".into());
    }
    let palette = if bpp == 8 {
        let start = 14 + header;
        let available = offset.saturating_sub(start) / 4;
        let mut palette = vec![[0u8, 0, 0, 255]; 256];
        for (i, slot) in palette.iter_mut().enumerate().take(available.min(256)) {
            let p = start + i * 4;
            *slot = [bytes[p + 2], bytes[p + 1], bytes[p], 255];
        }
        palette
    } else {
        Vec::new()
    };
    let mut out = vec![0u8; width * height * 4];
    for y in 0..height {
        let src_y = if top_down { y } else { height - 1 - y };
        let row = offset + src_y * stride;
        let dst = &mut out[y * width * 4..(y + 1) * width * 4];
        match bpp {
            8 => {
                for x in 0..width {
                    let c = palette[bytes[row + x] as usize];
                    dst[x * 4..x * 4 + 4].copy_from_slice(&c);
                }
            }
            24 => {
                for x in 0..width {
                    let p = row + x * 3;
                    dst[x * 4] = bytes[p + 2];
                    dst[x * 4 + 1] = bytes[p + 1];
                    dst[x * 4 + 2] = bytes[p];
                    dst[x * 4 + 3] = 255;
                }
            }
            _ => {
                for x in 0..width {
                    let p = row + x * 4;
                    dst[x * 4] = bytes[p + 2];
                    dst[x * 4 + 1] = bytes[p + 1];
                    dst[x * 4 + 2] = bytes[p];
                    dst[x * 4 + 3] = 255;
                }
            }
        }
    }
    Ok((width as u32, height as u32, out))
}

fn decode_tiff(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    use tiff::ColorType;
    let mut decoder = tiff::decoder::Decoder::new(std::io::Cursor::new(bytes))
        .map_err(|e| format!("invalid TIFF: {e}"))?;
    let (width, height) = decoder
        .dimensions()
        .map_err(|e| format!("invalid TIFF: {e}"))?;
    let color = decoder
        .colortype()
        .map_err(|e| format!("invalid TIFF: {e}"))?;
    let pixels = decoder
        .read_image()
        .map_err(|e| format!("cannot decode TIFF: {e}"))?;
    let n = (width as usize) * (height as usize);
    let mut out = Vec::with_capacity(n * 4);
    match (color, pixels) {
        (ColorType::Gray(8), tiff::decoder::DecodingResult::U8(v)) => {
            for &g in v.iter().take(n) {
                out.extend_from_slice(&[g, g, g, 255]);
            }
        }
        (ColorType::GrayA(8), tiff::decoder::DecodingResult::U8(v)) => {
            for p in v.chunks_exact(2).take(n) {
                out.extend_from_slice(&[p[0], p[0], p[0], p[1]]);
            }
        }
        (ColorType::RGB(8), tiff::decoder::DecodingResult::U8(v)) => {
            for p in v.chunks_exact(3).take(n) {
                out.extend_from_slice(&[p[0], p[1], p[2], 255]);
            }
        }
        (ColorType::RGBA(8), tiff::decoder::DecodingResult::U8(v)) => {
            out.extend_from_slice(&v[..v.len().min(n * 4)]);
        }
        (ColorType::Gray(16), tiff::decoder::DecodingResult::U16(v)) => {
            for &g in v.iter().take(n) {
                let g = (g >> 8) as u8;
                out.extend_from_slice(&[g, g, g, 255]);
            }
        }
        (ColorType::RGB(16), tiff::decoder::DecodingResult::U16(v)) => {
            for p in v.chunks_exact(3).take(n) {
                out.extend_from_slice(&[
                    (p[0] >> 8) as u8,
                    (p[1] >> 8) as u8,
                    (p[2] >> 8) as u8,
                    255,
                ]);
            }
        }
        (ColorType::RGBA(16), tiff::decoder::DecodingResult::U16(v)) => {
            for p in v.chunks_exact(4).take(n) {
                out.extend_from_slice(&[
                    (p[0] >> 8) as u8,
                    (p[1] >> 8) as u8,
                    (p[2] >> 8) as u8,
                    (p[3] >> 8) as u8,
                ]);
            }
        }
        (kind, _) => {
            return Err(format!("unsupported TIFF pixel format: {kind:?}"));
        }
    }
    if out.len() < n * 4 {
        return Err("truncated TIFF pixels".into());
    }
    Ok((width, height, out))
}

fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| format!("cannot write PNG: {e}"))?;
        writer
            .write_image_data(rgba)
            .map_err(|e| format!("cannot write PNG: {e}"))?;
    }
    Ok(out)
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

    #[test]
    fn svg_to_png_renders_at_target() {
        let svg =
            br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 100"><rect width="200" height="100" fill="red"/></svg>"#;
        let png = svg_to_png(svg, Some((400.0, 400.0))).unwrap();
        // Valid PNG header, fitted into the 400x400 box at its 2:1 aspect.
        assert_eq!(
            &png[0..8],
            &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]
        );
        assert_eq!(png_aspect(&png), Some(2.0));
        // Natural size when no target is given.
        let png = svg_to_png(svg, None).unwrap();
        assert_eq!(png_aspect(&png), Some(2.0));
        // Malformed SVG is an error.
        assert!(svg_to_png(b"<svg", None).is_err());
    }
}
