//! Layout measurements: spatial (ink coverage) and structural (alignment,
//! spacing, overlaps) facts about a deck's slides.
//!
//! `measure` reports numbers, never judgments. Boxes are turned into an "ink"
//! model (text extents are estimated from their content, images fill their
//! box, filled presets fill theirs, line-only presets only their border), then
//! rasterized into a grid so overlapping shapes are measured as a union.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

use crate::error::Result;
use crate::model::Project;
use crate::render;

/// Terminal characters are about twice as tall as wide.
const CELL_ASPECT: f64 = 2.0;
/// Heatmap columns for a 16:9 slide.
const BASE_COLS: usize = 64;
const REF_ASPECT: f64 = 16.0 / 9.0;
const MIN_COLS: usize = 32;
const MAX_COLS: usize = 96;
const MIN_ROWS: usize = 12;
const MAX_ROWS: usize = 48;

/// Average glyph advance as a fraction of the font size.
const CHAR_WIDTH_FACTOR: f64 = 0.5;
/// Line height as a multiple of the font size.
const LINE_HEIGHT_FACTOR: f64 = 1.2;
const DEFAULT_FONT_PT: f64 = 18.0;

/// A cell counts as covered above this ink fraction.
const INK_EPS: f64 = 0.01;
/// Visual intensity of each ink kind: text is the hottest, a filled shape is
/// flat. The heatmap (and zones/mass) reflect weighted density, so a large
/// shape glows tepid while dense text glows hot.
const INK_WEIGHT_TEXT: f64 = 1.0;
const INK_WEIGHT_IMAGE: f64 = 0.8;
const INK_WEIGHT_FILL: f64 = 0.3;
const INK_WEIGHT_LINE: f64 = 0.2;
/// Edges within this distance share a gridline.
const ALIGN_TOL: f64 = 0.05;
/// A child is "inside" a container within this slack.
const CONTAIN_TOL: f64 = 0.02;
/// A pair overlaps when their intersection exceeds this share of the smaller.
const OVERLAP_FRACTION: f64 = 0.10;
/// A shape covering this share of the slide is treated as background.
const BACKGROUND_FRACTION: f64 = 0.95;
/// Maximum entries listed per category in the report.
const MAX_LIST: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl Rect {
    fn area(&self) -> f64 {
        self.w.max(0.0) * self.h.max(0.0)
    }
    fn right(&self) -> f64 {
        self.x + self.w
    }
    fn bottom(&self) -> f64 {
        self.y + self.h
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Ink {
    Text,
    Image,
    Fill,
    Line,
    None,
}

#[derive(Debug, Clone)]
struct Item {
    rect: Rect,
    ink: Ink,
    text: String,
    font_pt: f64,
    label: String,
}

impl Item {
    fn is_background(&self) -> bool {
        self.ink == Ink::None
    }
}

/// A measured gap: (size, start, end) in inches along an axis.
type Gap = (f64, f64, f64);

/// One slide's measurements.
#[derive(Debug, Clone)]
pub struct SlideMeasure {
    pub slide: String,
    pub master: Option<String>,
    // Spatial
    pub empty_ratio: f64,
    pub mass_dx: f64,
    pub mass_dy: f64,
    pub margin_l: f64,
    pub margin_r: f64,
    pub margin_t: f64,
    pub margin_b: f64,
    pub zones: [[f64; 3]; 3],
    heatmap: Vec<String>,
    // Structural
    pub align_snap: f64,
    pub align_left: Vec<(f64, usize)>,
    pub align_right: Vec<(f64, usize)>,
    pub align_top: Vec<(f64, usize)>,
    pub align_bottom: Vec<(f64, usize)>,
    pub off_grid: Vec<String>,
    pub spacing_cv: Option<f64>,
    pub v_gaps: Vec<Gap>,
    pub h_gaps: Vec<Gap>,
    pub overlaps: Vec<String>,
}

/// A whole-deck measurement.
#[derive(Debug, Clone)]
pub struct Measurement {
    pub title: String,
    pub width: f64,
    pub height: f64,
    pub slides: Vec<SlideMeasure>,
}

/// Measure every slide of the project at `project_dir`.
pub fn measure(project_dir: &Path) -> Result<Measurement> {
    let project = Project::load(project_dir)?;
    let spec = render::spec_json(&project)?;
    let spec: Value = serde_json::from_str(&spec)
        .map_err(|e| miette::miette!("cannot parse the deck spec: {e}"))?;
    let title = project.main.presentation.title.clone();
    Ok(measure_spec(&spec, title))
}

/// Measure the slides of an already-parsed spec.
fn measure_spec(spec: &Value, title: String) -> Measurement {
    let width = spec.get("width").and_then(Value::as_f64).unwrap_or(13.333);
    let height = spec.get("height").and_then(Value::as_f64).unwrap_or(7.5);

    let mut masters: BTreeMap<String, Vec<Item>> = BTreeMap::new();
    if let Some(list) = spec.get("masters").and_then(Value::as_array) {
        for m in list {
            let name = m.get("name").and_then(Value::as_str).unwrap_or("");
            let items = m
                .get("objects")
                .and_then(Value::as_array)
                .map(|objects| master_items(objects))
                .unwrap_or_default();
            masters.insert(name.to_string(), items);
        }
    }

    let mut slides = Vec::new();
    if let Some(sections) = spec.get("sections").and_then(Value::as_array) {
        for section in sections {
            let Some(list) = section.get("slides").and_then(Value::as_array) else {
                continue;
            };
            for slide in list {
                slides.push(measure_slide(spec, slide, &masters, width, height));
            }
        }
    }

    Measurement {
        title,
        width,
        height,
        slides,
    }
}

// ---------------------------------------------------------------------------
// Parsing

fn num(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64)
}

fn run_text(run: &Value) -> &str {
    run.get("text").and_then(Value::as_str).unwrap_or("")
}

fn runs_text(runs: &Value) -> String {
    runs.as_array()
        .map(|list| list.iter().map(run_text).collect::<String>())
        .unwrap_or_default()
}

fn runs_font_pt(runs: &Value) -> Option<f64> {
    runs.as_array().and_then(|list| {
        list.iter().find_map(|run| {
            run.get("fontSize")
                .and_then(Value::as_f64)
                .or_else(|| run.get("options").and_then(|o| num(o, "fontSize")))
        })
    })
}

fn has_fill(opts: Option<&Value>) -> bool {
    opts.and_then(|o| o.get("fill"))
        .is_some_and(|f| !f.is_null())
}

fn has_line(opts: Option<&Value>) -> bool {
    opts.and_then(|o| o.get("line"))
        .is_some_and(|l| !l.is_null())
}

fn master_items(objects: &[Value]) -> Vec<Item> {
    let mut out = Vec::new();
    for (i, obj) in objects.iter().enumerate() {
        let ty = obj.get("type").and_then(Value::as_str).unwrap_or("");
        // Placeholders are represented by the slide shapes that fill them.
        if ty == "placeholder" {
            continue;
        }
        let opts = obj.get("options");
        let text = obj
            .get("runs")
            .map(runs_text)
            .or_else(|| obj.get("text").and_then(Value::as_str).map(String::from))
            .unwrap_or_default();
        let font_pt = obj
            .get("runs")
            .and_then(runs_font_pt)
            .or_else(|| opts.and_then(|o| num(o, "fontSize")))
            .unwrap_or(DEFAULT_FONT_PT);
        let ink = if ty == "image" {
            Ink::Image
        } else if !text.is_empty() {
            Ink::Text
        } else if has_fill(opts) {
            Ink::Fill
        } else if has_line(opts) {
            Ink::Line
        } else {
            Ink::None
        };
        out.push(Item {
            rect: Rect {
                x: num(obj, "x").unwrap_or(0.0),
                y: num(obj, "y").unwrap_or(0.0),
                w: num(obj, "w").unwrap_or(0.0),
                h: num(obj, "h").unwrap_or(0.0),
            },
            ink,
            text,
            font_pt,
            label: format!("{ty}#{}", i + 1),
        });
    }
    out
}

/// The geometry of a master placeholder named `name`, if any. Master
/// placeholders were dropped from the item list, so read them from the raw
/// master objects instead.
fn placeholder_rect(spec: &Value, master: &str, name: &str) -> Option<Rect> {
    let list = spec.get("masters").and_then(Value::as_array)?;
    let m = list
        .iter()
        .find(|m| m.get("name").and_then(Value::as_str) == Some(master))?;
    let objects = m.get("objects").and_then(Value::as_array)?;
    for obj in objects {
        if obj.get("type").and_then(Value::as_str) != Some("placeholder") {
            continue;
        }
        let ph = obj
            .get("options")
            .and_then(|o| o.get("placeholder"))
            .and_then(Value::as_str)?;
        if ph == name {
            return Some(Rect {
                x: num(obj, "x").unwrap_or(0.0),
                y: num(obj, "y").unwrap_or(0.0),
                w: num(obj, "w").unwrap_or(0.0),
                h: num(obj, "h").unwrap_or(0.0),
            });
        }
    }
    None
}

fn slide_items(spec: &Value, slide: &Value, master: &str) -> Vec<Item> {
    let mut out = Vec::new();
    let shapes = slide
        .get("shapes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for (i, sh) in shapes.iter().enumerate() {
        let kind = sh.get("kind").and_then(Value::as_str).unwrap_or("");
        let text = sh
            .get("runs")
            .map(runs_text)
            .or_else(|| sh.get("text").and_then(Value::as_str).map(String::from))
            .unwrap_or_default();
        let font_pt = sh
            .get("runs")
            .and_then(runs_font_pt)
            .or_else(|| num(sh, "fontSize"))
            .unwrap_or(DEFAULT_FONT_PT);
        let mut rect = Rect {
            x: num(sh, "x").unwrap_or(0.0),
            y: num(sh, "y").unwrap_or(0.0),
            w: num(sh, "w").unwrap_or(0.0),
            h: num(sh, "h").unwrap_or(0.0),
        };
        if let Some(name) = sh.get("placeholder").and_then(Value::as_str)
            && let Some(master_rect) = placeholder_rect(spec, master, name)
        {
            rect = master_rect;
        }
        let ink = match kind {
            "image" => Ink::Image,
            "text" => {
                if text.is_empty() {
                    Ink::None
                } else {
                    Ink::Text
                }
            }
            _ => {
                if has_fill(Some(sh)) {
                    Ink::Fill
                } else if has_line(Some(sh)) {
                    Ink::Line
                } else {
                    Ink::None
                }
            }
        };
        out.push(Item {
            rect,
            ink,
            text,
            font_pt,
            label: format!("{kind}#{}", i + 1),
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Ink grid

fn dims(
    aspect: f64,
    base: usize,
    min_c: usize,
    max_c: usize,
    min_r: usize,
    max_r: usize,
) -> (usize, usize) {
    let cols = ((base as f64) * aspect / REF_ASPECT)
        .round()
        .clamp(min_c as f64, max_c as f64) as usize;
    let rows = ((cols as f64) / (CELL_ASPECT * aspect))
        .round()
        .clamp(min_r as f64, max_r as f64) as usize;
    (cols, rows)
}

struct Grid {
    cols: usize,
    rows: usize,
    cells: Vec<f32>,
}

impl Grid {
    fn new(cols: usize, rows: usize) -> Self {
        Self {
            cols,
            rows,
            cells: vec![0.0; cols * rows],
        }
    }

    fn rasterize(&mut self, slide: Rect, r: Rect, weight: f64) {
        if r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        let cw = slide.w / self.cols as f64;
        let ch = slide.h / self.rows as f64;
        let x0 = ((r.x - slide.x) / cw).floor().max(0.0) as usize;
        let y0 = ((r.y - slide.y) / ch).floor().max(0.0) as usize;
        let x1 = (((r.right() - slide.x) / cw).ceil() as usize).min(self.cols);
        let y1 = (((r.bottom() - slide.y) / ch).ceil() as usize).min(self.rows);
        for row in y0..y1 {
            for col in x0..x1 {
                let cell = Rect {
                    x: slide.x + col as f64 * cw,
                    y: slide.y + row as f64 * ch,
                    w: cw,
                    h: ch,
                };
                let frac = overlap_area(cell, r) / (cw * ch);
                let value = weight * frac;
                let idx = row * self.cols + col;
                if value > self.cells[idx] as f64 {
                    self.cells[idx] = value as f32;
                }
            }
        }
    }

    fn value(&self, col: usize, row: usize) -> f64 {
        self.cells[row * self.cols + col] as f64
    }

    fn total_ink(&self) -> f64 {
        self.cells.iter().map(|c| *c as f64).sum()
    }

    fn covered(&self) -> usize {
        self.cells.iter().filter(|c| **c as f64 >= INK_EPS).count()
    }

    /// Ink bounding box in slide inches.
    fn ink_bbox(&self, slide: Rect) -> Option<Rect> {
        let cw = slide.w / self.cols as f64;
        let ch = slide.h / self.rows as f64;
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for row in 0..self.rows {
            for col in 0..self.cols {
                if self.value(col, row) >= INK_EPS {
                    x0 = x0.min(col as f64 * cw);
                    y0 = y0.min(row as f64 * ch);
                    x1 = x1.max((col + 1) as f64 * cw);
                    y1 = y1.max((row + 1) as f64 * ch);
                }
            }
        }
        if x1 < x0 {
            None
        } else {
            Some(Rect {
                x: slide.x + x0,
                y: slide.y + y0,
                w: x1 - x0,
                h: y1 - y0,
            })
        }
    }

    fn downsampled(&self, cols: usize, rows: usize) -> Grid {
        let mut out = Grid::new(cols, rows);
        for row in 0..rows {
            for col in 0..cols {
                let c0 = col * self.cols / cols;
                let c1 = ((col + 1) * self.cols / cols).max(c0 + 1);
                let r0 = row * self.rows / rows;
                let r1 = ((row + 1) * self.rows / rows).max(r0 + 1);
                let mut sum = 0.0;
                let mut n = 0usize;
                for r in r0..r1.min(self.rows) {
                    for c in c0..c1.min(self.cols) {
                        sum += self.value(c, r);
                        n += 1;
                    }
                }
                out.cells[row * cols + col] = if n == 0 { 0.0 } else { (sum / n as f64) as f32 };
            }
        }
        out
    }
}

fn overlap_area(a: Rect, b: Rect) -> f64 {
    let w = (a.right().min(b.right()) - a.x.max(b.x)).max(0.0);
    let h = (a.bottom().min(b.bottom()) - a.y.max(b.y)).max(0.0);
    w * h
}

/// The estimated ink rectangle of a text box: text extents from its content,
/// not the box.
fn text_ink(r: Rect, text: &str, font_pt: f64) -> Option<Rect> {
    let chars = text.chars().count() as f64;
    if chars == 0.0 || r.w <= 0.0 || r.h <= 0.0 {
        return None;
    }
    let char_w = CHAR_WIDTH_FACTOR * font_pt / 72.0;
    let line_h = LINE_HEIGHT_FACTOR * font_pt / 72.0;
    let text_w = chars * char_w;
    let lines = (text_w / r.w).ceil().max(1.0);
    let ink_w = text_w.min(r.w).max(0.01);
    let ink_h = (lines * line_h).min(r.h).max(0.01);
    Some(Rect {
        x: r.x,
        y: r.y,
        w: ink_w,
        h: ink_h,
    })
}

fn line_band(r: Rect) -> f64 {
    (r.w.min(r.h) * 0.05).clamp(0.01, 0.06)
}

// ---------------------------------------------------------------------------
// Slide measurement

fn measure_slide(
    spec: &Value,
    slide: &Value,
    masters: &BTreeMap<String, Vec<Item>>,
    width: f64,
    height: f64,
) -> SlideMeasure {
    let name = slide
        .get("file")
        .and_then(Value::as_str)
        .unwrap_or("slide")
        .to_string();
    let master = slide
        .get("master")
        .and_then(Value::as_str)
        .map(String::from);
    let slide_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: width,
        h: height,
    };

    let mut items: Vec<Item> = master
        .as_ref()
        .and_then(|m| masters.get(m))
        .cloned()
        .unwrap_or_default();
    items.extend(slide_items(spec, slide, master.as_deref().unwrap_or("")));

    // Full-bleed shapes are backgrounds/paper, not content.
    let slide_area = slide_rect.area();
    for item in &mut items {
        if item.rect.area() >= slide_area * BACKGROUND_FRACTION {
            item.ink = Ink::None;
        }
    }

    let aspect = width / height;
    let (gcols, grows) = dims(aspect, 96, 48, 160, 24, 90);
    let mut grid = Grid::new(gcols, grows);
    for item in &items {
        match item.ink {
            Ink::Text => {
                if let Some(r) = text_ink(item.rect, &item.text, item.font_pt) {
                    grid.rasterize(slide_rect, r, INK_WEIGHT_TEXT);
                }
            }
            Ink::Image => grid.rasterize(slide_rect, item.rect, INK_WEIGHT_IMAGE),
            Ink::Fill => grid.rasterize(slide_rect, item.rect, INK_WEIGHT_FILL),
            Ink::Line => {
                let b = line_band(item.rect);
                grid.rasterize(slide_rect, Rect { h: b, ..item.rect }, INK_WEIGHT_LINE);
                grid.rasterize(
                    slide_rect,
                    Rect {
                        y: item.rect.bottom() - b,
                        h: b,
                        ..item.rect
                    },
                    INK_WEIGHT_LINE,
                );
                grid.rasterize(slide_rect, Rect { w: b, ..item.rect }, INK_WEIGHT_LINE);
                grid.rasterize(
                    slide_rect,
                    Rect {
                        x: item.rect.right() - b,
                        w: b,
                        ..item.rect
                    },
                    INK_WEIGHT_LINE,
                );
            }
            Ink::None => {}
        }
    }

    // Spatial metrics.
    let total = (gcols * grows) as f64;
    let empty_ratio = 1.0 - grid.covered() as f64 / total;
    let ink = grid.total_ink();
    let (mut sx, mut sy) = (0.0, 0.0);
    for row in 0..grows {
        for col in 0..gcols {
            let v = grid.value(col, row);
            if v > 0.0 {
                sx += v * (col as f64 + 0.5) / gcols as f64;
                sy += v * (row as f64 + 0.5) / grows as f64;
            }
        }
    }
    let (mass_dx, mass_dy) = if ink > 0.0 {
        ((sx / ink - 0.5) * 2.0, (sy / ink - 0.5) * 2.0)
    } else {
        (0.0, 0.0)
    };
    let (margin_l, margin_r, margin_t, margin_b) = match grid.ink_bbox(slide_rect) {
        Some(b) => (b.x, width - b.right(), b.y, height - b.bottom()),
        None => (0.0, 0.0, 0.0, 0.0),
    };

    let mut zones = [[0.0f64; 3]; 3];
    for (zi, zone) in zones.iter_mut().enumerate() {
        for (zj, cell) in zone.iter_mut().enumerate() {
            let c0 = zj * gcols / 3;
            let c1 = ((zj + 1) * gcols / 3).max(c0 + 1);
            let r0 = zi * grows / 3;
            let r1 = ((zi + 1) * grows / 3).max(r0 + 1);
            let mut sum = 0.0;
            let mut n = 0usize;
            for row in r0..r1.min(grows) {
                for col in c0..c1.min(gcols) {
                    sum += grid.value(col, row);
                    n += 1;
                }
            }
            *cell = if n == 0 { 0.0 } else { sum / n as f64 };
        }
    }

    let (hcols, hrows) = dims(aspect, BASE_COLS, MIN_COLS, MAX_COLS, MIN_ROWS, MAX_ROWS);
    let heat = grid.downsampled(hcols, hrows);
    let heatmap = (0..hrows)
        .map(|row| {
            (0..hcols)
                .map(|col| {
                    let v = heat.value(col, row);
                    if v < 0.05 {
                        '.'
                    } else if v < 0.35 {
                        ':'
                    } else if v < 0.75 {
                        '*'
                    } else {
                        '#'
                    }
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>();

    // Structural metrics.
    let structure: Vec<&Item> = items.iter().filter(|i| !i.is_background()).collect();
    let parents = containment(&structure);
    let (align_snap, align_lines, off_grid) = alignment(&structure);
    let (spacing_cv, v_gaps, h_gaps) = spacing(&structure, &parents);
    let overlaps = overlaps(&structure, &parents);

    SlideMeasure {
        slide: name,
        master,
        empty_ratio,
        mass_dx,
        mass_dy,
        margin_l,
        margin_r,
        margin_t,
        margin_b,
        zones,
        heatmap,
        align_snap,
        align_left: align_lines.0,
        align_right: align_lines.1,
        align_top: align_lines.2,
        align_bottom: align_lines.3,
        off_grid,
        spacing_cv,
        v_gaps,
        h_gaps,
        overlaps,
    }
}

// ---------------------------------------------------------------------------
// Structure

/// Parent of each item: the smallest other box that fully contains it.
fn containment(items: &[&Item]) -> Vec<Option<usize>> {
    let mut parents = vec![None; items.len()];
    for (i, item) in items.iter().enumerate() {
        let mut best: Option<(usize, f64)> = None;
        for (j, other) in items.iter().enumerate() {
            if i == j {
                continue;
            }
            let contains = item.rect.x >= other.rect.x - CONTAIN_TOL
                && item.rect.y >= other.rect.y - CONTAIN_TOL
                && item.rect.right() <= other.rect.right() + CONTAIN_TOL
                && item.rect.bottom() <= other.rect.bottom() + CONTAIN_TOL
                && other.rect.area() > item.rect.area();
            if contains && best.is_none_or(|(_, a)| other.rect.area() < a) {
                best = Some((j, other.rect.area()));
            }
        }
        parents[i] = best.map(|(j, _)| j);
    }
    parents
}

fn is_ancestor(parents: &[Option<usize>], ancestor: usize, mut node: usize) -> bool {
    while let Some(p) = parents[node] {
        if p == ancestor {
            return true;
        }
        node = p;
    }
    false
}

fn cluster(mut values: Vec<f64>) -> Vec<(f64, usize)> {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut out: Vec<(f64, usize)> = Vec::new();
    for v in values {
        match out.last_mut() {
            Some((pos, n)) if (v - *pos).abs() <= ALIGN_TOL => {
                // Keep the running mean so a chain of near edges stays one line.
                *pos = (*pos * *n as f64 + v) / (*n as f64 + 1.0);
                *n += 1;
            }
            _ => out.push((v, 1)),
        }
    }
    out
}

type AlignLines = (
    Vec<(f64, usize)>,
    Vec<(f64, usize)>,
    Vec<(f64, usize)>,
    Vec<(f64, usize)>,
);

fn top_lines(mut lines: Vec<(f64, usize)>) -> Vec<(f64, usize)> {
    lines.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.partial_cmp(&b.0).unwrap()));
    lines.truncate(MAX_LIST);
    lines
}

fn alignment(items: &[&Item]) -> (f64, AlignLines, Vec<String>) {
    let left = cluster(items.iter().map(|i| i.rect.x).collect());
    let right = cluster(items.iter().map(|i| i.rect.right()).collect());
    let top = cluster(items.iter().map(|i| i.rect.y).collect());
    let bottom = cluster(items.iter().map(|i| i.rect.bottom()).collect());
    let all_sub = [&left, &right, &top, &bottom];
    // Gridlines proper: lines shared by at least two edges.
    let multi: Vec<&(f64, usize)> = all_sub
        .iter()
        .flat_map(|lines| lines.iter())
        .filter(|(_, n)| *n > 1)
        .collect();

    let total_edges = items.len() * 4;
    let mut off_count = 0usize;
    let mut off_grid = Vec::new();
    for item in items {
        for (kind, value, lines) in [
            ("left", item.rect.x, &left),
            ("right", item.rect.right(), &right),
            ("top", item.rect.y, &top),
            ("bottom", item.rect.bottom(), &bottom),
        ] {
            // On a gridline: an edge within tolerance of a shared line.
            let on = lines
                .iter()
                .any(|(pos, n)| *n > 1 && (pos - value).abs() <= ALIGN_TOL);
            if on {
                continue;
            }
            off_count += 1;
            // Only name the off-grid edges when the slide has an actual grid
            // to deviate from; otherwise every edge is "unique", not "off".
            if !multi.is_empty() && off_grid.len() < MAX_LIST {
                let nearest = multi
                    .iter()
                    .map(|(pos, _)| (pos - value).abs())
                    .fold(f64::MAX, f64::min);
                off_grid.push(format!(
                    "{} {} @ {:.2} (+{:.2} from nearest)",
                    item.label, kind, value, nearest
                ));
            }
        }
    }
    let align_snap = if total_edges == 0 {
        0.0
    } else {
        (total_edges - off_count) as f64 / total_edges as f64
    };

    let lines_for = |lines: &[(f64, usize)]| {
        let keep: Vec<(f64, usize)> = lines.iter().filter(|(_, n)| *n > 1).copied().collect();
        top_lines(keep)
    };

    (
        align_snap,
        (
            lines_for(&left),
            lines_for(&right),
            lines_for(&top),
            lines_for(&bottom),
        ),
        off_grid,
    )
}

fn spacing(items: &[&Item], parents: &[Option<usize>]) -> (Option<f64>, Vec<Gap>, Vec<Gap>) {
    let mut v_gaps = Vec::new();
    let mut h_gaps = Vec::new();
    let mut groups: BTreeMap<Option<usize>, Vec<usize>> = BTreeMap::new();
    for (i, parent) in parents.iter().enumerate() {
        groups.entry(*parent).or_default().push(i);
    }
    for members in groups.values() {
        // Vertical gaps between siblings that share a column.
        let mut by_y: Vec<usize> = members.clone();
        by_y.sort_by(|a, b| {
            items[*a]
                .rect
                .y
                .partial_cmp(&items[*b].rect.y)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for pair in by_y.windows(2) {
            let (a, b) = (items[pair[0]].rect, items[pair[1]].rect);
            if x_overlap(a, b) {
                let gap = b.y - a.bottom();
                if gap > INK_EPS {
                    v_gaps.push((gap, a.bottom(), b.y));
                }
            }
        }
        // Horizontal gaps between siblings that share a row.
        let mut by_x: Vec<usize> = members.clone();
        by_x.sort_by(|a, b| {
            items[*a]
                .rect
                .x
                .partial_cmp(&items[*b].rect.x)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for pair in by_x.windows(2) {
            let (a, b) = (items[pair[0]].rect, items[pair[1]].rect);
            if y_overlap(a, b) {
                let gap = b.x - a.right();
                if gap > INK_EPS {
                    h_gaps.push((gap, a.right(), b.x));
                }
            }
        }
    }

    let mut gaps: Vec<f64> = v_gaps.iter().map(|(g, _, _)| *g).collect();
    gaps.extend(h_gaps.iter().map(|(g, _, _)| *g));
    let cv = if gaps.len() >= 2 {
        let mean = gaps.iter().sum::<f64>() / gaps.len() as f64;
        if mean > 0.0 {
            let var = gaps.iter().map(|g| (g - mean).powi(2)).sum::<f64>() / gaps.len() as f64;
            Some(var.sqrt() / mean)
        } else {
            None
        }
    } else {
        None
    };

    v_gaps.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    h_gaps.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    v_gaps.truncate(MAX_LIST);
    h_gaps.truncate(MAX_LIST);
    (cv, v_gaps, h_gaps)
}

fn x_overlap(a: Rect, b: Rect) -> bool {
    (a.right().min(b.right()) - a.x.max(b.x)) > 0.01
}

fn y_overlap(a: Rect, b: Rect) -> bool {
    (a.bottom().min(b.bottom()) - a.y.max(b.y)) > 0.01
}

fn overlaps(items: &[&Item], parents: &[Option<usize>]) -> Vec<String> {
    let mut out = Vec::new();
    for i in 0..items.len() {
        for j in (i + 1)..items.len() {
            if is_ancestor(parents, i, j) || is_ancestor(parents, j, i) {
                continue;
            }
            let a = items[i].rect;
            let b = items[j].rect;
            let inter = overlap_area(a, b);
            if inter <= 0.0 {
                continue;
            }
            let smaller = a.area().min(b.area());
            if smaller > 0.0 && inter / smaller <= OVERLAP_FRACTION {
                continue;
            }
            if out.len() < MAX_LIST {
                let cx = (a.x.max(b.x) + a.right().min(b.right())) / 2.0;
                let cy = (a.y.max(b.y) + a.bottom().min(b.bottom())) / 2.0;
                out.push(format!(
                    "{} x {} @ ({:.2}, {:.2})",
                    items[i].label, items[j].label, cx, cy
                ));
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Rendering

impl Measurement {
    /// Render the deck measurement as Markdown.
    pub fn to_markdown(&self) -> String {
        let mut lines: Vec<String> = Vec::new();
        let title = if self.title.trim().is_empty() {
            "deck"
        } else {
            self.title.trim()
        };
        lines.push(format!("# gwen measure: {title}"));
        lines.push(String::new());
        lines.push(format!("- slides: {}", self.slides.len()));
        if !self.slides.is_empty() {
            let empty: f64 =
                self.slides.iter().map(|s| s.empty_ratio).sum::<f64>() / self.slides.len() as f64;
            let dx: f64 =
                self.slides.iter().map(|s| s.mass_dx).sum::<f64>() / self.slides.len() as f64;
            let dy: f64 =
                self.slides.iter().map(|s| s.mass_dy).sum::<f64>() / self.slides.len() as f64;
            lines.push(format!("- empty_ratio (avg): {empty:.2}"));
            lines.push(format!(
                "- mass_offset (avg): x {:+}%, y {:+}%",
                (dx * 100.0).round() as i64,
                (dy * 100.0).round() as i64
            ));
        }
        lines.push(format!("- size: {:.2} x {:.2} in", self.width, self.height));

        for slide in &self.slides {
            lines.push(String::new());
            lines.push(slide_heading(slide, self.width, self.height));
            lines.push(String::new());
            lines.push("### Spatial".into());
            lines.push(String::new());
            lines.push(format!("- empty_ratio: {:.2}", slide.empty_ratio));
            lines.push(format!(
                "- mass_offset: x {:+}%, y {:+}%",
                (slide.mass_dx * 100.0).round() as i64,
                (slide.mass_dy * 100.0).round() as i64
            ));
            lines.push(format!(
                "- margins_in: l {:.2}, r {:.2}, t {:.2}, b {:.2}",
                slide.margin_l, slide.margin_r, slide.margin_t, slide.margin_b
            ));
            lines.push(String::new());
            push_zone_block(&mut lines, &slide.zones);
            lines.push(String::new());
            push_fence(&mut lines, &slide.heatmap);

            lines.push(String::new());
            lines.push("### Structural".into());
            lines.push(String::new());
            lines.push(format!(
                "- align_snap: {:.2} ({} off-grid)",
                slide.align_snap,
                slide.off_grid.len()
            ));
            push_line_group(&mut lines, "left", "x-lines", &slide.align_left);
            push_line_group(&mut lines, "right", "x-lines", &slide.align_right);
            push_line_group(&mut lines, "top", "y-lines", &slide.align_top);
            push_line_group(&mut lines, "bottom", "y-lines", &slide.align_bottom);
            if !slide.off_grid.is_empty() {
                lines.push(format!("  - off-grid: {}", slide.off_grid.join("; ")));
            }
            match slide.spacing_cv {
                Some(cv) => lines.push(format!("- spacing_cv: {cv:.2}")),
                None => lines.push("- spacing_cv: -".into()),
            }
            push_gaps(&mut lines, "vertical gaps", &slide.v_gaps, 'y');
            push_gaps(&mut lines, "horizontal gaps", &slide.h_gaps, 'x');
            if slide.overlaps.is_empty() {
                lines.push("- overlaps: 0".into());
            } else {
                lines.push(format!("- overlaps: {}", slide.overlaps.len()));
                for o in &slide.overlaps {
                    lines.push(format!("  - {o}"));
                }
            }
        }

        // Trim trailing blank lines and end with exactly one newline.
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        let mut out = lines.join("\n");
        out.push('\n');
        out
    }

    /// Render the deck measurement as JSON.
    pub fn to_json(&self) -> String {
        let slides: Vec<Value> = self
            .slides
            .iter()
            .map(|s| {
                serde_json::json!({
                    "slide": s.slide,
                    "master": s.master,
                    "spatial": {
                        "empty_ratio": s.empty_ratio,
                        "mass_offset": { "x": s.mass_dx, "y": s.mass_dy },
                        "margins_in": {
                            "l": s.margin_l, "r": s.margin_r,
                            "t": s.margin_t, "b": s.margin_b,
                        },
                        "zones": s.zones,
                    },
                    "structural": {
                        "align_snap": s.align_snap,
                        "gridlines": {
                            "left": s.align_left,
                            "right": s.align_right,
                            "top": s.align_top,
                            "bottom": s.align_bottom,
                        },
                        "off_grid": s.off_grid,
                        "spacing_cv": s.spacing_cv,
                        "vertical_gaps": s.v_gaps,
                        "horizontal_gaps": s.h_gaps,
                        "overlaps": s.overlaps,
                    },
                })
            })
            .collect();
        let value = serde_json::json!({
            "title": self.title,
            "width": self.width,
            "height": self.height,
            "slides": slides,
        });
        serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".into())
    }
}

fn slide_heading(slide: &SlideMeasure, width: f64, height: f64) -> String {
    match &slide.master {
        Some(master) => format!(
            "## {} (master {}, {:.2}x{:.2} in)",
            slide.slide, master, width, height
        ),
        None => format!("## {} ({:.2}x{:.2} in)", slide.slide, width, height),
    }
}

fn push_zone_block(lines: &mut Vec<String>, zones: &[[f64; 3]; 3]) {
    lines.push("```text".into());
    lines.push("        left  center  right".into());
    for (i, zone) in zones.iter().enumerate() {
        let label = ["top", "mid", "bottom"][i];
        lines.push(format!(
            "{:<7} {:.2}   {:.2}    {:.2}",
            label, zone[0], zone[1], zone[2]
        ));
    }
    lines.push("```".into());
}

fn push_fence(lines: &mut Vec<String>, body: &[String]) {
    lines.push("```text".into());
    lines.extend(body.iter().cloned());
    lines.push("```".into());
}

fn push_line_group(lines: &mut Vec<String>, label: &str, kind: &str, lines_: &[(f64, usize)]) {
    if lines_.is_empty() {
        return;
    }
    let entries = lines_
        .iter()
        .map(|(pos, n)| format!("{pos:.2}x{n}"))
        .collect::<Vec<_>>()
        .join(", ");
    lines.push(format!("  - {label:<6} {kind}: {entries}"));
}

fn push_gaps(lines: &mut Vec<String>, label: &str, gaps: &[(f64, f64, f64)], axis: char) {
    if gaps.is_empty() {
        return;
    }
    let entries = gaps
        .iter()
        .map(|(g, a, b)| format!("{g:.2} @ {axis} {a:.2}->{b:.2}"))
        .collect::<Vec<_>>()
        .join(", ");
    lines.push(format!("  - {label}: {entries}"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn spec(slides: Vec<Value>) -> Value {
        json!({
            "width": 13.333,
            "height": 7.5,
            "masters": [],
            "sections": [{"title": "s", "slides": slides}],
        })
    }

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Value {
        json!({
            "kind": "shape", "type": "rect",
            "x": x, "y": y, "w": w, "h": h,
            "fill": { "color": "C7000A" },
        })
    }

    fn text(x: f64, y: f64, w: f64, h: f64, t: &str, font: f64) -> Value {
        json!({
            "kind": "text", "x": x, "y": y, "w": w, "h": h,
            "fontSize": font, "runs": [{"text": t}],
        })
    }

    fn first(slides: Vec<Value>) -> SlideMeasure {
        let m = measure_spec(&spec(slides), String::new());
        m.slides.into_iter().next().expect("one slide")
    }

    #[test]
    fn empty_slide_is_fully_empty() {
        let s = first(vec![json!({"file": "a.toml", "shapes": []})]);
        assert!((s.empty_ratio - 1.0).abs() < 1e-9);
    }

    #[test]
    fn full_bleed_rect_is_background_not_content() {
        let s = first(vec![
            json!({"file": "a.toml", "shapes": [rect(0.0, 0.0, 13.333, 7.5)]}),
        ]);
        assert!((s.empty_ratio - 1.0).abs() < 1e-9, "bg rect is paper");
    }

    #[test]
    fn long_text_has_more_ink_than_a_word() {
        let word = first(vec![json!({
            "file": "a.toml",
            "shapes": [text(0.5, 0.5, 10.0, 2.0, "OK", 48.0)],
        })]);
        let long = first(vec![json!({
            "file": "a.toml",
            "shapes": [text(0.5, 0.5, 10.0, 2.0, &"word ".repeat(120), 48.0)],
        })]);
        assert!(
            long.empty_ratio < word.empty_ratio,
            "more text means more ink: word={:.2} long={:.2}",
            word.empty_ratio,
            long.empty_ratio
        );
    }

    #[test]
    fn left_heavy_content_moves_mass_and_margin() {
        let s = first(vec![json!({
            "file": "a.toml",
            "shapes": [
                rect(0.3, 0.5, 4.0, 1.0),
                rect(0.3, 2.0, 4.0, 1.0),
                rect(0.3, 3.5, 4.0, 1.5),
            ],
        })]);
        assert!(s.mass_dx < -0.3, "content leans left: {:.2}", s.mass_dx);
        assert!(s.margin_r > 6.0, "big right margin: {:.2}", s.margin_r);
    }

    #[test]
    fn overlapping_siblings_are_reported() {
        let s = first(vec![json!({
            "file": "a.toml",
            "shapes": [
                rect(1.0, 1.0, 3.0, 2.0),
                rect(2.0, 1.5, 3.0, 2.0),
            ],
        })]);
        assert_eq!(s.overlaps.len(), 1, "one overlapping pair");
    }

    #[test]
    fn markdown_is_well_formed() {
        let m = measure_spec(
            &spec(vec![
                json!({"file": "a.toml", "shapes": [rect(0.3, 0.5, 4.0, 1.0)]}),
                json!({"file": "b.toml", "shapes": [text(1.0, 1.0, 5.0, 1.0, "hello", 20.0)]}),
            ]),
            "deck".into(),
        );
        let md = m.to_markdown();
        assert!(
            md.ends_with('\n') && !md.ends_with("\n\n"),
            "single trailing newline"
        );
        assert!(!md.contains('\r'), "no CR");
        let mut blank = 0usize;
        for line in md.lines() {
            assert!(
                !line.ends_with(' ') && !line.ends_with('\t'),
                "no trailing whitespace: {line:?}"
            );
            if line.is_empty() {
                blank += 1;
                assert!(blank <= 1, "no double blank lines");
            } else {
                blank = 0;
            }
        }
        // The JSON variant parses as a document.
        let json: Value = serde_json::from_str(&m.to_json()).expect("measurement JSON parses");
        assert_eq!(json["slides"].as_array().map(|a| a.len()), Some(2));
    }
}
