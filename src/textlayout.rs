//! Line layout for preview text.
//!
//! PowerPoint wraps text with the real font, hanging bullets, tab stops,
//! paragraph spacing and autofit. This module reproduces that with the same
//! fonts resvg draws with: harfrust shaping for advances, skrifa metrics for
//! line height, `unicode-linebreak` for break opportunities and a small bidi
//! pass for right-to-left paragraphs. The painter only draws the resulting
//! positioned spans.

use std::collections::HashMap;

use unicode_bidi::bidi_class;

use crate::fonts::Fonts;
use crate::scene::{Align, Bullet, Fit, Run, TextBlock, TextDirection, Valign};

const PT_PER_IN: f64 = 72.0;
const DEFAULT_TAB_IN: f64 = 1.0;
const MIN_SHRINK: f64 = 0.3;

#[derive(Debug, Clone)]
pub struct Layout {
    pub lines: Vec<Line>,
    /// Width of the widest line, in inches.
    pub width: f64,
    /// Content height including paragraph spacing, in inches.
    pub height: f64,
    /// Font scale applied for `fit: shrink`.
    pub scale: f64,
    /// New box width and height for `fit: resize`.
    pub resize: Option<(f64, f64)>,
    /// Approximated text features, reported per slide.
    pub approximations: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Line {
    /// Top of the line in the text frame.
    pub y: f64,
    pub height: f64,
    /// Baseline in the text frame.
    pub baseline: f64,
    /// Width of the text, excluding bullets.
    pub width: f64,
    /// Horizontal alignment offset applied to spans.
    pub x: f64,
    pub spans: Vec<Span>,
    /// Extra space per word for justified lines, in inches.
    pub word_spacing: f64,
    pub rtl: bool,
    /// Paragraph spacing carried on the first/last line, in inches.
    pub para_space_before: f64,
    pub para_space_after: f64,
    pub last_in_paragraph: bool,
}

#[derive(Debug, Clone)]
pub struct Span {
    pub text: String,
    pub run: usize,
    /// Absolute position in the text frame, in inches.
    pub x: f64,
    pub width: f64,
    /// Scaled point size.
    pub size: f64,
    /// Baseline shift in inches (positive raises).
    pub dy: f64,
    /// Scaled letter spacing in inches.
    pub letter_spacing: f64,
    /// Ascent and descent in inches, for highlights and underlines.
    pub ascent: f64,
    pub descent: f64,
    pub bullet: bool,
    pub rtl: bool,
}

/// Lay `block` out inside a `w` by `h` inch box.
pub fn layout(block: &TextBlock, w: f64, h: f64, fonts: &Fonts) -> Layout {
    let margin = block.margin;
    let (avail_w, avail_h) = match block.direction {
        TextDirection::Horz | TextDirection::WordArtVert => (
            (w - margin.left - margin.right).max(0.0),
            (h - margin.top - margin.bottom).max(0.0),
        ),
        TextDirection::Vert | TextDirection::Vert270 => (
            (h - margin.top - margin.bottom).max(0.0),
            (w - margin.left - margin.right).max(0.0),
        ),
    };

    if block.direction == TextDirection::WordArtVert {
        return word_art(block, avail_w, avail_h, fonts);
    }

    let mut result = layout_at(block, avail_w, avail_h, fonts, 1.0);
    if block.fit == Fit::Shrink && result.height > avail_h + 1e-6 {
        // Binary search the largest scale that still fits vertically.
        let mut lo = MIN_SHRINK;
        let mut hi = 1.0;
        let mut best = layout_at(block, avail_w, avail_h, fonts, MIN_SHRINK);
        for _ in 0..18 {
            let mid = (lo + hi) / 2.0;
            let candidate = layout_at(block, avail_w, avail_h, fonts, mid);
            if candidate.height <= avail_h + 1e-6 {
                lo = mid;
                best = candidate;
            } else {
                hi = mid;
            }
        }
        result = best;
        if result.height > avail_h + 1e-6 {
            result
                .approximations
                .push("text still overflows after shrinking to 30%".to_string());
        }
    }

    result.width = result
        .lines
        .iter()
        .map(|line| line.width)
        .fold(0.0, f64::max);

    if block.fit == Fit::Resize {
        // Horizontal text keeps its width while wrapping; without wrapping it
        // grows sideways. Vertical text swaps the flow and cross axes.
        result.resize = Some(match block.direction {
            TextDirection::Horz => {
                let width = if block.wrap {
                    w
                } else {
                    result.width + margin.left + margin.right
                };
                (width, result.height + margin.top + margin.bottom)
            }
            TextDirection::Vert | TextDirection::Vert270 => (
                result.height + margin.left + margin.right,
                result.width + margin.top + margin.bottom,
            ),
            TextDirection::WordArtVert => (w, h),
        });
    }

    // Vertical alignment moves the whole block inside the available height.
    let offset = match block.valign {
        Valign::Top => 0.0,
        Valign::Middle => (avail_h - result.height) / 2.0,
        Valign::Bottom => avail_h - result.height,
    };
    for line in &mut result.lines {
        line.y += offset;
        line.baseline += offset;
    }
    result
}

fn layout_at(block: &TextBlock, avail_w: f64, _avail_h: f64, fonts: &Fonts, scale: f64) -> Layout {
    let paragraphs = paragraphs(block);
    let mut lines: Vec<Line> = Vec::new();
    let mut approximations = Vec::new();
    let mut counters = [0usize; 9];
    let mut last_para: Option<usize> = None;
    let mut mixed_reported = false;
    let mut underline_reported = false;

    for (para_index, para) in paragraphs.iter().enumerate() {
        if last_para.is_some() {
            // No special action; paragraph spacing is carried on the lines.
        }
        last_para = Some(para_index);

        for run in &block.runs {
            if underline_reported {
                break;
            }
            if let Some(style) = &run.underline
                && style != "sng"
                && style != "single"
            {
                approximations.push(format!("underline style `{style}` drawn as a single line"));
                underline_reported = true;
            }
        }

        let first = &block.runs[para.first_run];
        let bullet = para.bullet.as_ref();
        let indent_in = bullet.map(Bullet::indent_in).unwrap_or(27.0 / PT_PER_IN);
        let level = para.level.min(8);
        let text_x = if bullet.is_some() {
            indent_in * (level as f64 + 1.0)
        } else {
            0.0
        };
        let bullet_x = if bullet.is_some() {
            indent_in * level as f64
        } else {
            0.0
        };
        let avail = (avail_w - text_x).max(0.1);

        let number_label = match bullet {
            Some(Bullet::Number { style, start, .. }) => {
                counters[level] += 1;
                for deeper in counters.iter_mut().skip(level + 1) {
                    *deeper = 0;
                }
                let value = start + counters[level] - 1;
                match number_text(style, value) {
                    Some(text) => Some(text),
                    None => {
                        approximations
                            .push(format!("numbering style `{style}` rendered as arabic"));
                        Some(format!("{value}."))
                    }
                }
            }
            _ => None,
        };

        let space_before = first.para_space_before.max(block.para_space_before);
        let space_after = first.para_space_after.max(block.para_space_after);

        let segments = segments(block, para);
        let mut paragraph_lines: Vec<Line> = Vec::new();
        for glyphs in &segments {
            let wrapped = wrap(glyphs, avail, block, fonts, scale);
            for line in wrapped {
                let metrics = line_metrics(block, para, &line.glyphs, fonts, scale);
                let mut made = make_line(block, para, &line.glyphs, metrics, fonts, scale);
                // Mirror RTL text before alignment so the visual order runs
                // right to left while each span keeps its logical string.
                if made.rtl {
                    let mut has_ltr = false;
                    for span in &made.spans {
                        if span.bullet {
                            continue;
                        }
                        if span.rtl {
                            if span_has_ltr(&span.text) {
                                has_ltr = true;
                            }
                        } else if span_has_rtl(&span.text) {
                            has_ltr = true;
                        }
                    }
                    if has_ltr && !mixed_reported {
                        approximations
                            .push("mixed-direction text uses visual span order".to_string());
                        mixed_reported = true;
                    }
                    for span in &mut made.spans {
                        if !span.bullet {
                            span.x = made.width - (span.x + span.width);
                        }
                    }
                }
                let effective_align = if made.rtl && block.align == Align::Left {
                    Align::Right
                } else {
                    block.align
                };
                made.x = match effective_align {
                    Align::Left => 0.0,
                    Align::Center => ((avail - made.width) / 2.0).max(0.0),
                    Align::Right => (avail - made.width).max(0.0),
                    Align::Justify => 0.0,
                };
                for span in &mut made.spans {
                    if !span.bullet {
                        span.x += text_x + made.x;
                    }
                }
                paragraph_lines.push(made);
            }
        }

        if paragraph_lines.is_empty() {
            paragraph_lines.push(empty_line(block, para, fonts, scale));
        }

        if let Some(label) = number_label {
            if let Some(line) = paragraph_lines.first_mut() {
                let run = para.first_run;
                let size = block.runs[run].size * scale;
                let width = measure(&block.runs[run], &label, fonts, scale);
                line.spans.insert(
                    0,
                    Span {
                        text: label,
                        run,
                        x: bullet_x,
                        width,
                        size,
                        dy: 0.0,
                        letter_spacing: block.runs[run].char_spacing * scale / PT_PER_IN,
                        ascent: fonts
                            .family_metrics(&block.runs[run].font, block.runs[run].bold, size)
                            .ascent
                            / PT_PER_IN,
                        descent: fonts
                            .family_metrics(&block.runs[run].font, block.runs[run].bold, size)
                            .descent
                            / PT_PER_IN,
                        bullet: true,
                        rtl: false,
                    },
                );
            }
        } else if let Some(Bullet::Char { text, .. }) = bullet
            && let Some(line) = paragraph_lines.first_mut()
        {
            let run = para.first_run;
            let size = block.runs[run].size * scale;
            line.spans.insert(
                0,
                Span {
                    text: text.clone(),
                    run,
                    x: bullet_x,
                    width: measure(&block.runs[run], text, fonts, scale),
                    size,
                    dy: 0.0,
                    letter_spacing: 0.0,
                    ascent: fonts
                        .family_metrics(&block.runs[run].font, block.runs[run].bold, size)
                        .ascent
                        / PT_PER_IN,
                    descent: fonts
                        .family_metrics(&block.runs[run].font, block.runs[run].bold, size)
                        .descent
                        / PT_PER_IN,
                    bullet: true,
                    rtl: false,
                },
            );
        }

        if let Some(first_line) = paragraph_lines.first_mut() {
            first_line.para_space_before = space_before / PT_PER_IN;
        }
        if let Some(last_line) = paragraph_lines.last_mut() {
            last_line.para_space_after = space_after / PT_PER_IN;
            last_line.last_in_paragraph = true;
        }
        // Justified lines stretch spaces, except the paragraph's last line.
        for line in &mut paragraph_lines {
            if block.align == Align::Justify && !line.last_in_paragraph {
                let spaces = line
                    .spans
                    .iter()
                    .filter(|s| !s.bullet)
                    .map(|s| s.text.chars().filter(|c| *c == ' ').count())
                    .sum::<usize>();
                if spaces > 0 && line.width + 1e-6 < avail {
                    line.word_spacing = (avail - line.width) / spaces as f64;
                }
            }
        }

        lines.extend(paragraph_lines);
    }

    // Stack lines.
    let mut y = 0.0;
    for line in &mut lines {
        y += line.para_space_before;
        line.y = y;
        line.baseline += y;
        y += line.height;
        y += line.para_space_after;
    }

    Layout {
        lines,
        width: 0.0,
        height: y,
        scale,
        resize: None,
        approximations,
    }
}

fn measure(run: &Run, text: &str, fonts: &Fonts, scale: f64) -> f64 {
    if text.is_empty() {
        return 0.0;
    }
    Fonts::pt_to_in(fonts.measure_spaced(
        text,
        &run.font,
        run.bold,
        run.italic,
        run.size * scale,
        run.char_spacing * scale,
    ))
}

fn span_has_ltr(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(
            bidi_class(c),
            unicode_bidi::BidiClass::L
                | unicode_bidi::BidiClass::EN
                | unicode_bidi::BidiClass::ES
                | unicode_bidi::BidiClass::ET
                | unicode_bidi::BidiClass::CS
                | unicode_bidi::BidiClass::ON
        )
    })
}

fn span_has_rtl(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(
            bidi_class(c),
            unicode_bidi::BidiClass::R | unicode_bidi::BidiClass::AL
        )
    })
}

fn first_strong_rtl(text: &str) -> Option<bool> {
    for c in text.chars() {
        match bidi_class(c) {
            unicode_bidi::BidiClass::R | unicode_bidi::BidiClass::AL => return Some(true),
            unicode_bidi::BidiClass::L => return Some(false),
            _ => {}
        }
    }
    None
}

// --- paragraph and glyph model -------------------------------------------

#[derive(Debug, Clone)]
struct Paragraph {
    first_run: usize,
    level: usize,
    bullet: Option<Bullet>,
    runs: Vec<usize>,
}

#[derive(Debug, Clone, Copy)]
struct Glyph {
    ch: char,
    run: usize,
}

fn paragraphs(block: &TextBlock) -> Vec<Paragraph> {
    let mut out = Vec::new();
    let mut runs = Vec::new();
    for (index, run) in block.runs.iter().enumerate() {
        runs.push(index);
        if run.break_line {
            out.push(make_paragraph(block, &runs));
            runs.clear();
        }
    }
    if !runs.is_empty() {
        out.push(make_paragraph(block, &runs));
    }
    if out.is_empty() {
        out.push(Paragraph {
            first_run: 0,
            level: 0,
            bullet: None,
            runs: vec![0],
        });
    }
    out
}

fn make_paragraph(block: &TextBlock, runs: &[usize]) -> Paragraph {
    let first_run = runs.first().copied().unwrap_or(0);
    let first = &block.runs[first_run];
    Paragraph {
        first_run,
        level: first.indent_level,
        bullet: first.bullet.clone().filter(|b| match b {
            Bullet::Char { text, .. } => !text.is_empty(),
            Bullet::Number { .. } => true,
        }),
        runs: runs.to_vec(),
    }
}

/// Split a paragraph into visual segments at `\n` and `softBreakBefore`.
fn segments(block: &TextBlock, para: &Paragraph) -> Vec<Vec<Glyph>> {
    let mut out: Vec<Vec<Glyph>> = Vec::new();
    let mut current: Vec<Glyph> = Vec::new();
    let mut started = false;
    for &index in &para.runs {
        let run = &block.runs[index];
        if run.soft_break_before && started {
            out.push(std::mem::take(&mut current));
        }
        for ch in run.text.chars() {
            if ch == '\n' {
                out.push(std::mem::take(&mut current));
                started = true;
            } else {
                current.push(Glyph { ch, run: index });
                started = true;
            }
        }
    }
    if !current.is_empty() || out.is_empty() {
        out.push(current);
    }
    out
}

fn is_space(g: &Glyph) -> bool {
    g.ch == ' ' || g.ch == '\t'
}

fn trim_end(glyphs: &[Glyph]) -> &[Glyph] {
    let mut end = glyphs.len();
    while end > 0 && is_space(&glyphs[end - 1]) {
        end -= 1;
    }
    &glyphs[..end]
}

// --- wrapping -------------------------------------------------------------

fn wrap(
    glyphs: &[Glyph],
    avail: f64,
    block: &TextBlock,
    fonts: &Fonts,
    scale: f64,
) -> Vec<WrappedLine> {
    if glyphs.is_empty() {
        return vec![WrappedLine { glyphs: Vec::new() }];
    }
    if !block.wrap {
        return vec![WrappedLine {
            glyphs: glyphs.to_vec(),
        }];
    }

    let text: String = glyphs
        .iter()
        .map(|g| if g.ch == '\t' { ' ' } else { g.ch })
        .collect();
    let mut by_byte = HashMap::new();
    let mut bytes = 0usize;
    for (index, g) in glyphs.iter().enumerate() {
        bytes += if g.ch == '\t' { 1 } else { g.ch.len_utf8() };
        by_byte.insert(bytes, index + 1);
    }
    let mut opportunities: Vec<usize> = unicode_linebreak::linebreaks(&text)
        .filter_map(|(offset, _)| by_byte.get(&offset).copied())
        .collect();
    opportunities.sort_unstable();
    opportunities.dedup();
    if opportunities.last().copied() != Some(glyphs.len()) {
        opportunities.push(glyphs.len());
    }

    let mut lines = Vec::new();
    let mut start = 0usize;
    while start < glyphs.len() {
        let mut end = start;
        for &opp in opportunities.iter().filter(|opp| **opp > start) {
            let candidate = trim_end(&glyphs[start..opp]);
            if width(candidate, block, fonts, scale) <= avail + 1e-9 {
                end = opp;
            } else {
                break;
            }
        }
        if end == start {
            // A single unbreakable chunk is wider than the line: split it at
            // the last glyph that fits.
            let mut take = 0usize;
            while take < glyphs.len() - start
                && width(&glyphs[start..start + take + 1], block, fonts, scale) <= avail + 1e-9
            {
                take += 1;
            }
            if take == 0 {
                take = 1;
            }
            end = start + take;
        }
        lines.push(WrappedLine {
            glyphs: trim_end(&glyphs[start..end]).to_vec(),
        });
        start = end;
        // Drop the break space at the start of the next line.
        while start < glyphs.len() && glyphs[start].ch == ' ' {
            start += 1;
        }
    }
    if lines.is_empty() {
        lines.push(WrappedLine { glyphs: Vec::new() });
    }
    lines
}

struct WrappedLine {
    glyphs: Vec<Glyph>,
}

/// Width of a glyph sequence starting at x=0, honoring tab stops.
fn width(glyphs: &[Glyph], block: &TextBlock, fonts: &Fonts, scale: f64) -> f64 {
    let mut x = 0.0;
    let mut group = String::new();
    let mut group_run = None;
    for glyph in glyphs {
        if glyph.ch == '\t' {
            flush_group(&mut group, &mut group_run, &mut x, block, fonts, scale);
            x = next_tab(x, &block.tab_stops);
        } else {
            if group_run.is_some_and(|run| run != glyph.run) {
                flush_group(&mut group, &mut group_run, &mut x, block, fonts, scale);
            }
            group_run = Some(glyph.run);
            group.push(glyph.ch);
        }
    }
    flush_group(&mut group, &mut group_run, &mut x, block, fonts, scale);
    x
}

fn flush_group(
    group: &mut String,
    group_run: &mut Option<usize>,
    x: &mut f64,
    block: &TextBlock,
    fonts: &Fonts,
    scale: f64,
) {
    if group.is_empty() {
        return;
    }
    if let Some(run) = *group_run {
        *x += measure(&block.runs[run], group, fonts, scale);
    }
    group.clear();
    *group_run = None;
}

fn next_tab(x: f64, stops: &[f64]) -> f64 {
    let mut next = stops
        .iter()
        .copied()
        .filter(|stop| *stop > x + 1e-6)
        .fold(f64::INFINITY, f64::min);
    if !next.is_finite() {
        next = (x / DEFAULT_TAB_IN + 1.0).floor() * DEFAULT_TAB_IN;
        if next <= x + 1e-6 {
            next += DEFAULT_TAB_IN;
        }
    }
    next
}

// --- lines ----------------------------------------------------------------

struct LineMetrics {
    descent: f64,
    height: f64,
}

fn line_metrics(
    block: &TextBlock,
    para: &Paragraph,
    glyphs: &[Glyph],
    fonts: &Fonts,
    scale: f64,
) -> LineMetrics {
    let mut ascent = 0.0f64;
    let mut descent = 0.0f64;
    let mut natural = 0.0f64;
    let mut seen = vec![false; block.runs.len()];
    let mut any = false;
    for glyph in glyphs {
        if glyph.run >= seen.len() || seen[glyph.run] {
            continue;
        }
        seen[glyph.run] = true;
        any = true;
        let run = &block.runs[glyph.run];
        let size = run.size * scale;
        let metrics = fonts.family_metrics(&run.font, run.bold, size);
        let shift = run.baseline / 100.0 * size;
        ascent = ascent.max(metrics.ascent + shift.max(0.0));
        descent = descent.max(metrics.descent + (-shift).max(0.0));
        natural = natural.max(metrics.line_height());
    }
    if !any {
        let run = &block.runs[para.first_run.min(block.runs.len() - 1)];
        let size = run.size * scale;
        let metrics = fonts.family_metrics(&run.font, run.bold, size);
        let shift = run.baseline / 100.0 * size;
        ascent = metrics.ascent + shift.max(0.0);
        descent = metrics.descent + (-shift).max(0.0);
        natural = metrics.line_height();
    }
    let first = &block.runs[para.first_run];
    let exact = first.line_spacing.or(block.line_spacing);
    let multiple = first.line_spacing_multiple.or(block.line_spacing_multiple);
    let height = if let Some(pt) = exact {
        pt / PT_PER_IN
    } else if let Some(mult) = multiple {
        natural.max(ascent + descent) / PT_PER_IN * mult
    } else {
        natural.max(ascent + descent) / PT_PER_IN
    };
    let _ = ascent;
    LineMetrics {
        descent: descent / PT_PER_IN,
        height: height.max(0.01),
    }
}

fn make_line(
    block: &TextBlock,
    para: &Paragraph,
    glyphs: &[Glyph],
    metrics: LineMetrics,
    fonts: &Fonts,
    scale: f64,
) -> Line {
    let mut spans = Vec::new();
    let mut x = 0.0f64;
    let mut i = 0usize;
    while i < glyphs.len() {
        let run_index = glyphs[i].run;
        let run = &block.runs[run_index];
        if glyphs[i].ch == '\t' {
            x = next_tab(x, &block.tab_stops);
            i += 1;
            continue;
        }
        let start = i;
        while i < glyphs.len() && glyphs[i].run == run_index && glyphs[i].ch != '\t' {
            i += 1;
        }
        let text: String = glyphs[start..i].iter().map(|g| g.ch).collect();
        let width = measure(run, &text, fonts, scale);
        let rtl = first_strong_rtl(&text).unwrap_or(false);
        let metrics = fonts.family_metrics(&run.font, run.bold, run.size * scale);
        spans.push(Span {
            text,
            run: run_index,
            x,
            width,
            size: run.size * scale,
            dy: -(run.baseline / 100.0 * run.size * scale) / PT_PER_IN,
            letter_spacing: run.char_spacing * scale / PT_PER_IN,
            ascent: metrics.ascent / PT_PER_IN,
            descent: metrics.descent / PT_PER_IN,
            bullet: false,
            rtl,
        });
        x += width;
    }

    let all_text: String = spans.iter().map(|s| s.text.as_str()).collect();
    let base_rtl = block.rtl || first_strong_rtl(&all_text) == Some(true);
    if base_rtl {
        for span in &mut spans {
            if first_strong_rtl(&span.text).is_none() {
                span.rtl = true;
            }
        }
    }

    let _ = para;
    Line {
        y: 0.0,
        height: metrics.height,
        baseline: metrics.height - metrics.descent,
        width: x,
        x: 0.0,
        spans,
        word_spacing: 0.0,
        rtl: base_rtl,
        para_space_before: 0.0,
        para_space_after: 0.0,
        last_in_paragraph: false,
    }
}

fn empty_line(block: &TextBlock, para: &Paragraph, fonts: &Fonts, scale: f64) -> Line {
    let metrics = line_metrics(block, para, &[], fonts, scale);
    Line {
        y: 0.0,
        height: metrics.height,
        baseline: metrics.height - metrics.descent,
        width: 0.0,
        x: 0.0,
        spans: Vec::new(),
        word_spacing: 0.0,
        rtl: block.rtl,
        para_space_before: 0.0,
        para_space_after: 0.0,
        last_in_paragraph: false,
    }
}

// --- word art -------------------------------------------------------------

fn word_art(block: &TextBlock, avail_w: f64, avail_h: f64, fonts: &Fonts) -> Layout {
    let mut lines = Vec::new();
    let mut approximations = vec!["stacked text (`wordArtVert`) is approximated".to_string()];
    if block.fit == Fit::Resize {
        approximations.push("autofit resize is not applied to stacked text".to_string());
    }
    let para_list = paragraphs(block);
    let mut y = 0.0;
    for para in &para_list {
        if para.bullet.is_some() {
            approximations.push("bullets are omitted from stacked text".to_string());
        }
        for &run_index in &para.runs {
            let run = &block.runs[run_index];
            for ch in run.text.chars() {
                if ch == '\n' {
                    y += run.size / PT_PER_IN * 0.5;
                    continue;
                }
                let size = run.size;
                let metrics = fonts.family_metrics(&run.font, run.bold, size);
                let height = metrics.line_height() / PT_PER_IN;
                if ch == ' ' {
                    y += height;
                    continue;
                }
                let width = measure(run, &ch.to_string(), fonts, 1.0);
                let x = match block.align {
                    Align::Left => 0.0,
                    Align::Center | Align::Justify => ((avail_w - width) / 2.0).max(0.0),
                    Align::Right => (avail_w - width).max(0.0),
                };
                lines.push(Line {
                    y,
                    height,
                    baseline: y + metrics.ascent / PT_PER_IN,
                    width,
                    x,
                    spans: vec![Span {
                        text: ch.to_string(),
                        run: run_index,
                        x,
                        width,
                        size,
                        dy: 0.0,
                        letter_spacing: 0.0,
                        ascent: metrics.ascent / PT_PER_IN,
                        descent: metrics.descent / PT_PER_IN,
                        bullet: false,
                        rtl: false,
                    }],
                    word_spacing: 0.0,
                    rtl: false,
                    para_space_before: 0.0,
                    para_space_after: 0.0,
                    last_in_paragraph: false,
                });
                y += height;
            }
        }
    }
    let offset = match block.valign {
        Valign::Top => 0.0,
        Valign::Middle => (avail_h - y).max(0.0) / 2.0,
        Valign::Bottom => (avail_h - y).max(0.0),
    };
    for line in &mut lines {
        line.y += offset;
        line.baseline += offset;
    }
    Layout {
        width: lines.iter().map(|l| l.width).fold(0.0, f64::max),
        lines,
        height: y,
        scale: 1.0,
        resize: None,
        approximations,
    }
}

// --- numbering ------------------------------------------------------------

fn number_text(style: &str, value: usize) -> Option<String> {
    let number = match style {
        "arabicPeriod" | "arabicParenR" | "arabicParenBoth" => value.to_string(),
        "alphaLcPeriod" | "alphaLcParenR" => alpha(value, false),
        "alphaUcPeriod" | "alphaUcParenR" => alpha(value, true),
        "romanLcPeriod" | "romanLcParenR" => roman(value).to_lowercase(),
        "romanUcPeriod" | "romanUcParenR" => roman(value),
        _ => return None,
    };
    let suffix = if style.ends_with("ParenBoth") || style.ends_with("ParenR") {
        ")"
    } else {
        "."
    };
    let prefix = if style.ends_with("ParenBoth") {
        "("
    } else {
        ""
    };
    Some(format!("{prefix}{number}{suffix}"))
}

fn alpha(mut value: usize, upper: bool) -> String {
    value = value.max(1);
    let mut out = String::new();
    while value > 0 {
        let rem = (value - 1) % 26;
        out.insert(0, (b'a' + rem as u8) as char);
        value = (value - 1) / 26;
    }
    if upper { out.to_uppercase() } else { out }
}

fn roman(mut value: usize) -> String {
    const VALUES: &[(usize, &str)] = &[
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut out = String::new();
    for &(n, glyph) in VALUES {
        while value >= n {
            out.push_str(glyph);
            value -= n;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;
    use crate::scene::Margins;

    fn run(text: &str, size: f64) -> Run {
        Run {
            text: text.into(),
            font: "DejaVu Sans".into(),
            size,
            bold: false,
            italic: false,
            underline: None,
            strike: false,
            color: Color::BLACK,
            highlight: None,
            alpha: 1.0,
            char_spacing: 0.0,
            baseline: 0.0,
            bullet: None,
            indent_level: 0,
            break_line: false,
            soft_break_before: false,
            line_spacing: None,
            line_spacing_multiple: None,
            para_space_before: 0.0,
            para_space_after: 0.0,
            hyperlink: false,
            outline: None,
            glow: None,
        }
    }

    fn block(runs: Vec<Run>) -> TextBlock {
        TextBlock {
            margin: Margins::default_insets(),
            align: Align::Left,
            valign: Valign::Top,
            wrap: true,
            fit: Fit::None,
            direction: TextDirection::Horz,
            rtl: false,
            runs,
            line_spacing: None,
            line_spacing_multiple: None,
            para_space_before: 0.0,
            para_space_after: 0.0,
            tab_stops: Vec::new(),
        }
    }

    #[test]
    fn wraps_long_text_and_preserves_words() {
        let fonts = Fonts::load(&std::env::temp_dir().join("gwen-layout-fonts"));
        let mut r = run("the quick brown fox jumps over the lazy dog", 18.0);
        r.break_line = true;
        let b = block(vec![r]);
        let out = layout(&b, 2.0, 5.0, &fonts);
        assert!(out.lines.len() > 1, "{:?}", out.lines.len());
        assert!(out.height > 0.0);
        for line in &out.lines {
            assert!(line.width <= 1.9, "line too wide: {}", line.width);
        }
    }

    #[test]
    fn bullet_indents_follow_pptxgenjs_defaults() {
        let fonts = Fonts::load(&std::env::temp_dir().join("gwen-layout-fonts"));
        let mut first = run("one", 18.0);
        first.bullet = Some(Bullet::Char {
            text: "\u{2022}".into(),
            indent_pt: 27.0,
        });
        first.break_line = true;
        let mut second = run("two", 18.0);
        second.indent_level = 1;
        second.bullet = Some(Bullet::Char {
            text: "\u{2022}".into(),
            indent_pt: 27.0,
        });
        let out = layout(&block(vec![first, second]), 4.0, 3.0, &fonts);
        assert_eq!(out.lines.len(), 2);
        let bullet0 = out.lines[0].spans.iter().find(|s| s.bullet).unwrap();
        let text0 = out.lines[0].spans.iter().find(|s| !s.bullet).unwrap();
        assert!((bullet0.x - 0.0).abs() < 1e-6);
        assert!((text0.x - 27.0 / 72.0).abs() < 1e-6);
        let bullet1 = out.lines[1].spans.iter().find(|s| s.bullet).unwrap();
        let text1 = out.lines[1].spans.iter().find(|s| !s.bullet).unwrap();
        assert!((bullet1.x - 27.0 / 72.0).abs() < 1e-6);
        assert!((text1.x - 2.0 * 27.0 / 72.0).abs() < 1e-6);
    }

    #[test]
    fn resize_reports_the_new_box() {
        let fonts = Fonts::load(&std::env::temp_dir().join("gwen-layout-fonts"));

        let mut wrapping = block(vec![run(
            "one two three four five six seven eight nine ten",
            18.0,
        )]);
        wrapping.fit = Fit::Resize;
        let out = layout(&wrapping, 2.0, 0.5, &fonts);
        let (width, height) = out.resize.unwrap();
        assert!((width - 2.0).abs() < 1e-9, "wrapping keeps its width");
        assert!(height > 0.5, "wrapping grows the height");

        let mut no_wrap = block(vec![run("one two three four five", 18.0)]);
        no_wrap.fit = Fit::Resize;
        no_wrap.wrap = false;
        let out = layout(&no_wrap, 1.0, 0.5, &fonts);
        let (width, height) = out.resize.unwrap();
        assert!(width > 1.0, "without wrapping the width grows: {width}");
        assert!(height > 0.0);

        let mut vertical = block(vec![run("abc", 18.0)]);
        vertical.fit = Fit::Resize;
        vertical.direction = TextDirection::Vert270;
        let out = layout(&vertical, 1.0, 3.0, &fonts);
        let (width, height) = out.resize.unwrap();
        assert!(height > width, "vertical text grows along its flow axis");
    }

    #[test]
    fn numbering_styles_are_stable() {
        assert_eq!(number_text("arabicPeriod", 3).unwrap(), "3.");
        assert_eq!(number_text("alphaLcPeriod", 28).unwrap(), "ab.");
        assert_eq!(number_text("romanUcPeriod", 4).unwrap(), "IV.");
        assert!(number_text("unknown", 1).is_none());
    }
}
