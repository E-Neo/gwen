//! Paint a resolved scene as SVG.
//!
//! The SVG uses points as user units (`1in = 72`), so text metrics, stroke
//! widths and effect distances map directly. Shape outlines come from
//! `presets::path` in local inches and are drawn under a `scale(72)` group.
//! resvg turns the document into the PNG the vision model reads.

use std::fmt::Write as _;

use crate::color::Color;
use crate::fonts::Fonts;
use crate::presets::{self, GeomOpts};
use crate::scene::{
    Background, Glow, Item, ItemKind, Scene, Shadow, ShadowKind, Stroke, TextDirection,
};
use crate::textlayout;

const PT_PER_IN: f64 = 72.0;

/// Render one scene to an SVG document.
pub fn render(scene: &Scene, fonts: &Fonts) -> String {
    let w = scene.width * PT_PER_IN;
    let h = scene.height * PT_PER_IN;
    let mut defs = Defs::default();
    let mut body = String::new();

    // slide background: an explicit color/image sits on the default white.
    body.push_str(&format!(
        "<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"#ffffff\"/>",
        num(w),
        num(h)
    ));
    match &scene.background {
        Background::None => {}
        Background::Color(color) => {
            body.push_str(&format!(
                "<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"{}\"{} />",
                num(w),
                num(h),
                hex(*color),
                opacity_attr("fill-opacity", color.a)
            ));
        }
        Background::Image { data, transparency } => {
            body.push_str(&format!(
                "<image x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\" href=\"{}\"{}/>",
                num(w),
                num(h),
                escape_attr(data),
                opacity_attr("opacity", 1.0 - transparency / 100.0)
            ));
        }
    }

    for item in &scene.items {
        paint_item(item, &mut body, &mut defs, fonts);
    }

    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{wpt}pt\" height=\"{hpt}pt\" viewBox=\"0 0 {wpt} {hpt}\">\
         <defs>{defs}</defs>{body}</svg>",
        wpt = num(w),
        hpt = num(h),
        defs = defs.inner,
        body = body,
    )
}

#[derive(Default)]
struct Defs {
    inner: String,
    next: usize,
}

impl Defs {
    fn id(&mut self, prefix: &str) -> String {
        self.next += 1;
        format!("{prefix}{}", self.next)
    }

    fn push(&mut self, text: &str) {
        self.inner.push_str(text);
    }

    /// Build a filter for shadow and glow. `unit` is points per user unit
    /// (1.0 for text, 1/72 for paths drawn in inches).
    fn effect(
        &mut self,
        shadow: Option<&Shadow>,
        glow: Option<&Glow>,
        unit: f64,
    ) -> Option<String> {
        if shadow.is_none() && glow.is_none() {
            return None;
        }
        let id = self.id("fx");
        let mut x = String::new();
        let _ = write!(
            x,
            "<filter id=\"{id}\" x=\"-100%\" y=\"-100%\" width=\"300%\" height=\"300%\" filterUnits=\"objectBoundingBox\" color-interpolation-filters=\"sRGB\">"
        );
        let mut merged_input = "SourceGraphic".to_string();
        if let Some(glow) = glow {
            let blur = (glow.size * unit).max(0.01);
            let _ = write!(
                x,
                "<feGaussianBlur in=\"SourceAlpha\" stdDeviation=\"{}\" result=\"glowBlur\"/>\
                 <feFlood flood-color=\"{}\" flood-opacity=\"{}\" result=\"glowColor\"/>\
                 <feComposite in=\"glowColor\" in2=\"glowBlur\" operator=\"in\" result=\"glow\"/>\
                 <feMerge result=\"withGlow\"><feMergeNode in=\"glow\"/><feMergeNode in=\"SourceGraphic\"/></feMerge>",
                num(blur),
                hex(glow.color),
                num(glow.color.a * glow.opacity)
            );
            merged_input = "withGlow".to_string();
        }
        if let Some(shadow) = shadow {
            let angle = shadow.angle.to_radians();
            let dx = shadow.distance * angle.cos() * unit;
            let dy = shadow.distance * angle.sin() * unit;
            let blur = (shadow.blur * unit).max(0.01);
            match shadow.kind {
                ShadowKind::Outer => {
                    let _ = write!(
                        x,
                        "<feGaussianBlur in=\"SourceAlpha\" stdDeviation=\"{}\" result=\"shadowBlur\"/>\
                         <feOffset dx=\"{}\" dy=\"{}\" in=\"shadowBlur\" result=\"shadowOffset\"/>\
                         <feFlood flood-color=\"{}\" flood-opacity=\"{}\" result=\"shadowColor\"/>\
                         <feComposite in=\"shadowColor\" in2=\"shadowOffset\" operator=\"in\" result=\"shadow\"/>",
                        num(blur),
                        num(dx),
                        num(dy),
                        hex(shadow.color),
                        num(shadow.color.a * shadow.opacity)
                    );
                }
                ShadowKind::Inner => {
                    let _ = write!(
                        x,
                        "<feComponentTransfer in=\"SourceAlpha\" result=\"inverse\"><feFuncA type=\"table\" tableValues=\"1 0\"/></feComponentTransfer>\
                         <feGaussianBlur in=\"inverse\" stdDeviation=\"{}\" result=\"innerBlur\"/>\
                         <feOffset dx=\"{}\" dy=\"{}\" in=\"innerBlur\" result=\"innerOffset\"/>\
                         <feComposite in=\"innerOffset\" in2=\"SourceAlpha\" operator=\"in\" result=\"inner\"/>\
                         <feFlood flood-color=\"{}\" flood-opacity=\"{}\" result=\"innerColor\"/>\
                         <feComposite in=\"innerColor\" in2=\"inner\" operator=\"in\" result=\"shadow\"/>",
                        num(blur),
                        num(dx),
                        num(dy),
                        hex(shadow.color),
                        num(shadow.color.a * shadow.opacity)
                    );
                }
            }
            let _ = write!(
                x,
                "<feMerge><feMergeNode in=\"shadow\"/><feMergeNode in=\"{}\"/></feMerge>",
                merged_input
            );
        } else {
            let _ = write!(
                x,
                "<feMerge><feMergeNode in=\"{}\"/></feMerge>",
                merged_input
            );
        }
        x.push_str("</filter>");
        self.push(&x);
        Some(id)
    }

    fn marker(&mut self, kind: &str, color: Color, start: bool) -> String {
        let id = self.id(if start { "ms" } else { "me" });
        let body = match kind {
            "diamond" => "<path d=\"M5 0 L10 5 L5 10 L0 5 Z\"".to_string(),
            "oval" => "<circle cx=\"5\" cy=\"5\" r=\"5\"".to_string(),
            "stealth" => "<path d=\"M0 0 L10 5 L0 10 L3 5 Z\"".to_string(),
            "arrow" => {
                "<path d=\"M0 0 L10 5 L0 10\" fill=\"none\" stroke-width=\"1.4\"".to_string()
            }
            _ => "<path d=\"M0 0 L10 5 L0 10 Z\"".to_string(),
        };
        let orient = if start { "auto-start-reverse" } else { "auto" };
        let fill = if kind == "arrow" {
            format!(" stroke=\"{}\"", hex(color))
        } else {
            format!(" fill=\"{}\"", hex(color))
        };
        self.push(&format!(
            "<marker id=\"{id}\" viewBox=\"0 0 10 10\" refX=\"10\" refY=\"5\" markerWidth=\"4\" markerHeight=\"4\" markerUnits=\"strokeWidth\" orient=\"{orient}\">{body}{fill}/></marker>"
        ));
        id
    }
}

fn paint_item(item: &Item, out: &mut String, defs: &mut Defs, fonts: &Fonts) {
    let transform = item_transform(item);
    if let Some(transform) = &transform {
        let _ = write!(out, "<g transform=\"{}\">", escape_attr(transform));
    }
    match &item.kind {
        ItemKind::Shape { preset, geom } => {
            paint_shape(item, preset, geom, out, defs);
        }
        ItemKind::Image { .. } => {
            paint_image(item, out, defs);
        }
        ItemKind::Text(block) => {
            paint_text(item, block, out, defs, fonts);
        }
    }
    if transform.is_some() {
        out.push_str("</g>");
    }
}

fn item_transform(item: &Item) -> Option<String> {
    if item.rotate == 0.0 && !item.flip_h && !item.flip_v {
        return None;
    }
    let cx = (item.rect.x + item.rect.w / 2.0) * PT_PER_IN;
    let cy = (item.rect.y + item.rect.h / 2.0) * PT_PER_IN;
    let mut out = format!("translate({} {})", num(cx), num(cy));
    if item.rotate != 0.0 {
        let _ = write!(out, " rotate({})", num(item.rotate));
    }
    if item.flip_h || item.flip_v {
        let _ = write!(
            out,
            " scale({} {})",
            if item.flip_h { -1 } else { 1 },
            if item.flip_v { -1 } else { 1 }
        );
    }
    let _ = write!(out, " translate({} {})", num(-cx), num(-cy));
    Some(out)
}

fn paint_shape(item: &Item, preset: &str, geom: &GeomOpts, out: &mut String, defs: &mut Defs) {
    let fx = defs.effect(item.shadow.as_ref(), None, 1.0 / PT_PER_IN);
    let filter = fx
        .as_ref()
        .map(|id| format!(" filter=\"url(#{id})\""))
        .unwrap_or_default();
    let path = presets::path(
        preset,
        item.rect.w.max(0.0001),
        item.rect.h.max(0.0001),
        geom,
    );
    let mut marker_attrs = String::new();
    if let Some(line) = &item.line {
        if let Some(head) = &line.head {
            let id = defs.marker(head, line.color, true);
            let _ = write!(marker_attrs, " marker-start=\"url(#{id})\"");
        }
        if let Some(tail) = &line.tail {
            let id = defs.marker(tail, line.color, false);
            let _ = write!(marker_attrs, " marker-end=\"url(#{id})\"");
        }
    }
    let paint = paint_attrs(
        item.fill,
        item.line.as_ref(),
        1.0 / PT_PER_IN,
        item.even_odd,
    );
    let _ = write!(
        out,
        "<g transform=\"translate({} {}) scale({})\"{}>",
        num(item.rect.x * PT_PER_IN),
        num(item.rect.y * PT_PER_IN),
        num(PT_PER_IN),
        filter
    );
    match path {
        Some(d) if !d.trim().is_empty() => {
            let _ = write!(
                out,
                "<path d=\"{}\"{} {} />",
                escape_attr(&d),
                paint,
                marker_attrs
            );
        }
        _ => {
            let _ = write!(
                out,
                "<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\"{} {} />",
                num(item.rect.w.max(0.0001)),
                num(item.rect.h.max(0.0001)),
                paint,
                marker_attrs
            );
        }
    }
    out.push_str("</g>");
}

fn paint_attrs(fill: Option<Color>, line: Option<&Stroke>, unit: f64, even_odd: bool) -> String {
    let mut out = String::from(" ");
    match fill {
        Some(color) => {
            let _ = write!(
                out,
                "fill=\"{}\"{}",
                hex(color),
                opacity_attr("fill-opacity", color.a)
            );
        }
        None => out.push_str("fill=\"none\""),
    }
    if even_odd {
        out.push_str(" fill-rule=\"evenodd\"");
    }
    if let Some(line) = line {
        let _ = write!(
            out,
            " stroke=\"{}\" stroke-width=\"{}\" stroke-linejoin=\"round\" stroke-linecap=\"round\"{}",
            hex(line.color),
            num((line.width * unit).max(0.0)),
            opacity_attr("stroke-opacity", line.color.a)
        );
        if let Some(dash) = &line.dash {
            let pattern = dash_array(dash, (line.width).max(0.5));
            if !pattern.is_empty() {
                let scaled: Vec<String> = pattern.iter().map(|value| num(value * unit)).collect();
                let _ = write!(out, " stroke-dasharray=\"{}\"", scaled.join(" "));
            }
        }
    }
    out
}

fn dash_array(kind: &str, width: f64) -> Vec<f64> {
    let w = width.max(0.5);
    match kind {
        "solid" => Vec::new(),
        "dash" => vec![4.0 * w, 3.0 * w],
        "dashDot" => vec![4.0 * w, 3.0 * w, 1.0 * w, 3.0 * w],
        "lgDash" => vec![8.0 * w, 3.0 * w],
        "lgDashDot" => vec![8.0 * w, 3.0 * w, 1.0 * w, 3.0 * w],
        "lgDashDotDot" => vec![8.0 * w, 3.0 * w, 1.0 * w, 3.0 * w, 1.0 * w, 3.0 * w],
        "sysDash" => vec![3.0 * w, 1.0 * w],
        "sysDot" => vec![1.0 * w, 1.0 * w],
        _ => Vec::new(),
    }
}

fn paint_image(item: &Item, out: &mut String, defs: &mut Defs) {
    let ItemKind::Image {
        data,
        sizing,
        crop,
        ellipse,
        transparency,
    } = &item.kind
    else {
        return;
    };
    let (sizing, crop, ellipse, transparency) = (sizing.as_str(), *crop, *ellipse, *transparency);
    let fx = defs.effect(item.shadow.as_ref(), None, 1.0);
    let filter = fx
        .as_ref()
        .map(|id| format!(" filter=\"url(#{id})\""))
        .unwrap_or_default();
    let rect = item.rect;
    let (target, image_rect, preserve) = if let Some(crop) = crop {
        (
            crate::scene::Rect {
                x: rect.x,
                y: rect.y,
                w: crop.w,
                h: crop.h,
            },
            crate::scene::Rect {
                x: rect.x - crop.x,
                y: rect.y - crop.y,
                w: rect.w,
                h: rect.h,
            },
            "none",
        )
    } else {
        let preserve = if sizing == "cover" {
            "xMidYMid slice"
        } else {
            "xMidYMid meet"
        };
        (rect, rect, preserve)
    };
    let clip_id = defs.id("clip");
    let clip = if ellipse {
        format!(
            "<clipPath id=\"{clip_id}\"><ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\"/></clipPath>",
            num((target.x + target.w / 2.0) * PT_PER_IN),
            num((target.y + target.h / 2.0) * PT_PER_IN),
            num((target.w / 2.0) * PT_PER_IN),
            num((target.h / 2.0) * PT_PER_IN)
        )
    } else {
        format!(
            "<clipPath id=\"{clip_id}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/></clipPath>",
            num(target.x * PT_PER_IN),
            num(target.y * PT_PER_IN),
            num(target.w * PT_PER_IN),
            num(target.h * PT_PER_IN)
        )
    };
    defs.push(&clip);
    let _ = write!(
        out,
        "<g clip-path=\"url(#{clip_id})\"{}{}><image x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"{}\" href=\"{}\"/></g>",
        filter,
        opacity_attr("opacity", 1.0 - transparency / 100.0),
        num(image_rect.x * PT_PER_IN),
        num(image_rect.y * PT_PER_IN),
        num(image_rect.w * PT_PER_IN),
        num(image_rect.h * PT_PER_IN),
        preserve,
        escape_attr(data)
    );
}

fn paint_text(
    item: &Item,
    block: &crate::scene::TextBlock,
    out: &mut String,
    defs: &mut Defs,
    fonts: &Fonts,
) {
    let mut normalized = block.clone();
    textlayout::normalize_fonts(&mut normalized, fonts);
    let block = &normalized;
    let rect = item.rect;
    let fx = defs.effect(item.shadow.as_ref(), None, 1.0);
    let item_filter = fx
        .as_ref()
        .map(|id| format!(" filter=\"url(#{id})\""))
        .unwrap_or_default();
    let has_box = item.fill.is_some() || item.line.is_some();
    // A plain text box can carry a fill and border; a shape-preset text box
    // already drew them below the text. The shape shadow belongs to that box
    // when it has visible geometry, otherwise PowerPoint's shape effect reads
    // as a shadow of the text itself.
    if has_box {
        let paint = paint_attrs(item.fill, item.line.as_ref(), 1.0, false);
        let _ = write!(
            out,
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}{} />",
            num(rect.x * PT_PER_IN),
            num(rect.y * PT_PER_IN),
            num(rect.w.max(0.0) * PT_PER_IN),
            num(rect.h.max(0.0) * PT_PER_IN),
            paint,
            item_filter
        );
    }

    let text_filter = if has_box { "" } else { item_filter.as_str() };
    if text_filter.is_empty() {
        let _ = write!(out, "<g>");
    } else {
        let _ = write!(out, "<g{}>", text_filter);
    }

    let layout = textlayout::layout(block, rect.w, rect.h, fonts);
    let (transform, origin_x, origin_y) = text_frame(item, block);
    if let Some(transform) = &transform {
        let _ = write!(out, "<g transform=\"{}\">", escape_attr(transform));
    } else {
        out.push_str("<g>");
    }
    let ox = origin_x * PT_PER_IN;
    let oy = origin_y * PT_PER_IN;

    for line in &layout.lines {
        // Highlight boxes sit behind their spans.
        for span in &line.spans {
            if span.bullet {
                continue;
            }
            let run = &block.runs[span.run];
            if let Some(color) = run.highlight {
                let y = line.baseline + span.dy - span.ascent;
                let _ = write!(
                    out,
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"{} />",
                    num(ox + span.x * PT_PER_IN),
                    num(oy + y * PT_PER_IN),
                    num(span.width * PT_PER_IN),
                    num((span.ascent + span.descent) * PT_PER_IN),
                    hex(color),
                    opacity_attr("fill-opacity", color.a)
                );
            }
        }
        for span in &line.spans {
            let run = &block.runs[span.run];
            let filter = defs.effect(None, run.glow.as_ref(), 1.0);
            let filter_attr = filter
                .as_ref()
                .map(|id| format!(" filter=\"url(#{id})\""))
                .unwrap_or_default();
            let anchor = if span.rtl { "end" } else { "start" };
            let direction = if span.rtl { " direction=\"rtl\"" } else { "" };
            let mut attrs = String::new();
            let _ = write!(
                attrs,
                "x=\"{}\" y=\"{}\" font-family=\"{}\" font-size=\"{}\" fill=\"{}\"{} text-anchor=\"{}\"{}",
                num(ox + span.x * PT_PER_IN),
                num(oy + (line.baseline + span.dy) * PT_PER_IN),
                escape_attr(&run.font),
                num(span.size),
                hex(run.color),
                opacity_attr("fill-opacity", run.color.a * run.alpha),
                anchor,
                direction
            );
            if run.bold {
                attrs.push_str(" font-weight=\"bold\"");
            }
            if run.italic {
                attrs.push_str(" font-style=\"italic\"");
            }
            if span.letter_spacing.abs() > 1e-9 {
                let _ = write!(
                    attrs,
                    " letter-spacing=\"{}\"",
                    num(span.letter_spacing * PT_PER_IN)
                );
            }
            if line.word_spacing.abs() > 1e-9 {
                let _ = write!(
                    attrs,
                    " word-spacing=\"{}\"",
                    num(line.word_spacing * PT_PER_IN)
                );
            }
            if let Some(outline) = run.outline {
                let _ = write!(
                    attrs,
                    " stroke=\"{}\" stroke-width=\"{}\" stroke-linejoin=\"round\" paint-order=\"stroke\"",
                    hex(outline.color),
                    num(outline.size.max(0.1))
                );
            }
            attrs.push_str(&filter_attr);
            let _ = write!(
                out,
                "<text {} xml:space=\"preserve\">{}</text>",
                attrs,
                escape_text(&span.text)
            );
            // Underlines and strikethroughs are drawn explicitly: resvg's
            // text-decoration support is not uniform across fonts.
            let baseline = oy + (line.baseline + span.dy) * PT_PER_IN;
            let x0 = ox + span.x * PT_PER_IN;
            let x1 = x0 + span.width * PT_PER_IN;
            if run.underline.is_some() || run.hyperlink {
                let y = baseline + span.size * 0.09;
                let thickness = (span.size / 16.0).max(0.75);
                let _ = write!(
                    out,
                    "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"{}\"{} />",
                    num(x0),
                    num(y),
                    num(x1),
                    num(y),
                    hex(run.color),
                    num(thickness),
                    opacity_attr("stroke-opacity", run.color.a * run.alpha)
                );
            }
            if run.strike {
                let y = baseline - span.size * 0.28;
                let thickness = (span.size / 16.0).max(0.75);
                let _ = write!(
                    out,
                    "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"{}\"{} />",
                    num(x0),
                    num(y),
                    num(x1),
                    num(y),
                    hex(run.color),
                    num(thickness),
                    opacity_attr("stroke-opacity", run.color.a * run.alpha)
                );
            }
        }
    }
    out.push_str("</g>");
    out.push_str("</g>");
}

/// Where the laid-out text frame starts and how it is rotated into the box.
fn text_frame(item: &Item, block: &crate::scene::TextBlock) -> (Option<String>, f64, f64) {
    let rect = item.rect;
    let m = block.margin;
    match block.direction {
        TextDirection::Horz | TextDirection::WordArtVert => (None, rect.x + m.left, rect.y + m.top),
        TextDirection::Vert => {
            let ox = rect.x + rect.w - m.right;
            let oy = rect.y + m.top;
            (
                Some(format!(
                    "translate({} {}) rotate(90)",
                    num(ox * PT_PER_IN),
                    num(oy * PT_PER_IN)
                )),
                0.0,
                0.0,
            )
        }
        TextDirection::Vert270 => {
            let ox = rect.x + m.left;
            let oy = rect.y + rect.h - m.bottom;
            (
                Some(format!(
                    "translate({} {}) rotate(-90)",
                    num(ox * PT_PER_IN),
                    num(oy * PT_PER_IN)
                )),
                0.0,
                0.0,
            )
        }
    }
}

fn hex(color: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b)
}

fn opacity_attr(name: &str, value: f64) -> String {
    if (value - 1.0).abs() < 1e-9 {
        String::new()
    } else {
        format!(" {}=\"{}\"", name, num(value.clamp(0.0, 1.0)))
    }
}

fn num(value: f64) -> String {
    let rounded = (value * 10_000.0).round() / 10_000.0;
    if rounded == 0.0 {
        "0".to_string()
    } else {
        let mut text = format!("{rounded}");
        if text.contains('.') {
            while text.ends_with('0') {
                text.pop();
            }
            if text.ends_with('.') {
                text.pop();
            }
        }
        text
    }
}

fn escape_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_attr(text: &str) -> String {
    escape_text(text)
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_markup() {
        assert_eq!(escape_text("a < b & c"), "a &lt; b &amp; c");
        assert_eq!(escape_attr("\"x\""), "&quot;x&quot;");
    }

    #[test]
    fn filters_are_addressable() {
        let mut defs = Defs::default();
        let shadow = Shadow {
            kind: ShadowKind::Outer,
            color: Color::BLACK,
            opacity: 0.5,
            blur: 8.0,
            angle: 45.0,
            distance: 4.0,
            rotate_with_shape: false,
        };
        let id = defs.effect(Some(&shadow), None, 1.0).unwrap();
        assert_eq!(id, "fx1");
        assert!(defs.inner.contains("<filter id=\"fx1\""), "{}", defs.inner);
        assert!(defs.inner.contains("</filter>"));
    }

    #[test]
    fn numbers_are_compact() {
        assert_eq!(num(1.5), "1.5");
        assert_eq!(num(72.0), "72");
        assert_eq!(num(-0.00001), "0");
    }
}
