//! Geometry for every pptxgenjs shape preset.
//!
//! A preview needs the *outline* of each preset, not OOXML. Each builder
//! returns an SVG path in the item's local coordinates (`0..w`, `0..h`, in
//! inches). Presets share families (rectangles, stars, arrows, callouts,
//! flowcharts, ...), so the builders are parameterised and the id match is a
//! thin dispatch table.
//!
//! `custGeom` is special: its path is the user's `points` list, in the same
//! local inches, following pptxgenjs's `moveTo` / `close` / `curve` model.

use std::f64::consts::PI;

/// Options that change a preset's outline.
#[derive(Debug, Clone, Default)]
pub struct GeomOpts {
    /// `rectRadius`, as a fraction of the shorter side (pptxgenjs units).
    pub rect_radius: Option<f64>,
    /// `arcThicknessRatio`, as a fraction of the shorter side.
    pub arc_thickness_ratio: Option<f64>,
    /// `angleRange`, in degrees, clockwise from 3 o'clock.
    pub angle_range: Option<(f64, f64)>,
    /// `custGeom` points.
    pub points: Option<Vec<Point>>,
}

#[derive(Debug, Clone, Default)]
pub struct Point {
    pub x: f64,
    pub y: f64,
    pub move_to: bool,
    pub close: bool,
    pub curve: Option<Curve>,
}

#[derive(Debug, Clone)]
pub enum Curve {
    Arc {
        hr: f64,
        wr: f64,
        st_ang: f64,
        sw_ang: f64,
    },
    Cubic {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
    },
    Quadratic {
        x1: f64,
        y1: f64,
    },
}

/// The outline of `id` in local inches, or `None` for an unknown id.
pub fn path(id: &str, w: f64, h: f64, opts: &GeomOpts) -> Option<String> {
    let w = w.max(0.0001);
    let h = h.max(0.0001);
    let min = w.min(h);
    let r = opts.rect_radius.unwrap_or(0.16667).clamp(0.0, 0.5) * min;
    let angle = opts.angle_range.unwrap_or((0.0, 360.0));
    let thickness = opts.arc_thickness_ratio.unwrap_or(0.25).clamp(0.01, 0.99);
    let p = |pts: &[(f64, f64)]| poly(pts);
    let path = match id {
        "rect" => p(&[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]),
        "roundRect" => round_rect(0.0, 0.0, w, h, [r, r, r, r]),
        "round1Rect" => round_rect(0.0, 0.0, w, h, [0.0, r, 0.0, 0.0]),
        "round2SameRect" => round_rect(0.0, 0.0, w, h, [r, r, 0.0, 0.0]),
        "round2DiagRect" => round_rect(0.0, 0.0, w, h, [r, 0.0, r, 0.0]),
        "snip1Rect" => snip_rect(0.0, 0.0, w, h, [0.0, r, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0]),
        "snip2SameRect" => snip_rect(0.0, 0.0, w, h, [r, r, 0.0, 0.0], [1.0, 1.0, 0.0, 0.0]),
        "snip2DiagRect" => snip_rect(0.0, 0.0, w, h, [r, 0.0, r, 0.0], [1.0, 0.0, 1.0, 0.0]),
        "snipRoundRect" => snip_rect(0.0, 0.0, w, h, [r, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0]),
        "plaque" => plaque(0.0, 0.0, w, h, r),
        "plaqueTabs" => plaque(0.0, h * 0.08, w, h * 0.84, r * 0.6),
        "frame" => frame(0.0, 0.0, w, h, 0.25),
        "halfFrame" => {
            let t = 0.25;
            poly_evenodd(&[
                &[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)],
                &[
                    (0.0, h * t),
                    (w * t, h * t),
                    (w * t, h * (1.0 - t)),
                    (0.0, h * (1.0 - t)),
                ],
            ])
        }
        "corner" => {
            let t = 0.3;
            poly_evenodd(&[
                &[(0.0, 0.0), (w, 0.0), (w * t, h * t), (w * t, h), (0.0, h)],
                &[(w * 0.1, h * 0.1), (w * 0.1, h * 0.9), (w * 0.9, h * 0.1)],
            ])
        }
        "cornerTabs" => {
            let t = 0.25;
            poly(&[
                (0.0, 0.0),
                (w * 0.5, 0.0),
                (w * 0.5, h * t),
                (w, h * t),
                (w, h),
                (0.0, h),
                (0.0, h * (1.0 - t)),
                (w * (1.0 - t), h * (1.0 - t)),
                (w * (1.0 - t), 0.0),
                (w * t, 0.0),
                (w * t, h * 0.5),
                (0.0, h * 0.5),
            ])
        }
        "squareTabs" => poly(&[
            (w * 0.1, 0.0),
            (w * 0.9, 0.0),
            (w * 0.9, h * 0.1),
            (w, h * 0.1),
            (w, h * 0.9),
            (w * 0.9, h * 0.9),
            (w * 0.9, h),
            (w * 0.1, h),
            (w * 0.1, h * 0.9),
            (0.0, h * 0.9),
            (0.0, h * 0.1),
            (w * 0.1, h * 0.1),
        ]),
        "diagStripe" => poly_evenodd(&[
            &[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)],
            &[
                (w * 0.15, 0.0),
                (w * 0.35, 0.0),
                (w * 0.85, h),
                (w * 0.65, h),
            ],
        ]),

        "ellipse" => ellipse(w / 2.0, h / 2.0, w / 2.0, h / 2.0),
        "triangle" => p(&[(w / 2.0, 0.0), (w, h), (0.0, h)]),
        "rtTriangle" => p(&[(0.0, 0.0), (0.0, h), (w, h)]),
        "diamond" => p(&[(w / 2.0, 0.0), (w, h / 2.0), (w / 2.0, h), (0.0, h / 2.0)]),
        "parallelogram" => p(&[(w * 0.25, 0.0), (w, 0.0), (w * 0.75, h), (0.0, h)]),
        "trapezoid" => p(&[(w * 0.25, 0.0), (w * 0.75, 0.0), (w, h), (0.0, h)]),
        "nonIsoscelesTrapezoid" => p(&[(w * 0.2, 0.0), (w * 0.8, 0.0), (w, h), (0.0, h)]),
        "pentagon" => regular(w / 2.0, h / 2.0, w / 2.0, h / 2.0, 5, -90.0),
        "hexagon" => regular(w / 2.0, h / 2.0, w / 2.0, h / 2.0, 6, 0.0),
        "heptagon" => regular(w / 2.0, h / 2.0, w / 2.0, h / 2.0, 7, -90.0),
        "octagon" => regular(w / 2.0, h / 2.0, w / 2.0, h / 2.0, 8, -67.5),
        "decagon" => regular(w / 2.0, h / 2.0, w / 2.0, h / 2.0, 10, -90.0),
        "dodecagon" => regular(w / 2.0, h / 2.0, w / 2.0, h / 2.0, 12, -90.0),
        "star4" => star(w / 2.0, h / 2.0, w / 2.0, h / 2.0, 4, 0.375),
        "star5" => star(w / 2.0, h / 2.0, w / 2.0, h / 2.0, 5, 0.382),
        "star6" => star(w / 2.0, h / 2.0, w / 2.0, h / 2.0, 6, 0.288),
        "star7" => star(w / 2.0, h / 2.0, w / 2.0, h / 2.0, 7, 0.346),
        "star8" => star(w / 2.0, h / 2.0, w / 2.0, h / 2.0, 8, 0.384),
        "star10" => star(w / 2.0, h / 2.0, w / 2.0, h / 2.0, 10, 0.4),
        "star12" => star(w / 2.0, h / 2.0, w / 2.0, h / 2.0, 12, 0.375),
        "star16" => star(w / 2.0, h / 2.0, w / 2.0, h / 2.0, 16, 0.375),
        "star24" => star(w / 2.0, h / 2.0, w / 2.0, h / 2.0, 24, 0.375),
        "star32" => star(w / 2.0, h / 2.0, w / 2.0, h / 2.0, 32, 0.375),
        "plus" => plus(w, h, 0.25),
        "mathPlus" => plus(w, h, 0.235),
        "mathMinus" => p(&[(0.0, h * 0.4), (w, h * 0.4), (w, h * 0.6), (0.0, h * 0.6)]),
        "mathMultiply" => {
            let t = 0.16;
            poly(&[
                (w * 0.5, h * (0.5 - t)),
                (w * (1.0 - t), 0.0),
                (w, h * t),
                (w * (0.5 + t), h * 0.5),
                (w, h * (1.0 - t)),
                (w * (1.0 - t), h),
                (w * 0.5, h * (0.5 + t)),
                (w * t, h),
                (0.0, h * (1.0 - t)),
                (w * (0.5 - t), h * 0.5),
                (0.0, h * t),
                (w * t, 0.0),
            ])
        }
        "mathDivide" => format!(
            "{} {} {}",
            p(&[
                (0.0, h * 0.42),
                (w, h * 0.42),
                (w, h * 0.58),
                (0.0, h * 0.58)
            ]),
            ellipse(w * 0.5, h * 0.18, w * 0.1, h * 0.1),
            ellipse(w * 0.5, h * 0.82, w * 0.1, h * 0.1)
        ),
        "mathEqual" => format!(
            "{} {}",
            p(&[(0.0, h * 0.3), (w, h * 0.3), (w, h * 0.42), (0.0, h * 0.42)]),
            p(&[(0.0, h * 0.58), (w, h * 0.58), (w, h * 0.7), (0.0, h * 0.7)])
        ),
        "mathNotEqual" => format!(
            "{} {} {}",
            p(&[(0.0, h * 0.3), (w, h * 0.3), (w, h * 0.42), (0.0, h * 0.42)]),
            p(&[(0.0, h * 0.58), (w, h * 0.58), (w, h * 0.7), (0.0, h * 0.7)]),
            p(&[(w * 0.2, h), (w * 0.3, h), (w * 0.8, 0.0), (w * 0.7, 0.0)])
        ),

        "rightArrow" => arrow_right(w, h),
        "leftArrow" => mirror_h(&arrow_right(w, h), w),
        "upArrow" => rotate90(&arrow_right(h, w), w, h),
        "downArrow" => rotate270(&arrow_right(h, w), w, h),
        "leftRightArrow" => left_right_arrow(w, h),
        "upDownArrow" => rotate90(&left_right_arrow(h, w), w, h),
        "quadArrow" => quad_arrow(w, h),
        "leftUpArrow" => left_up_arrow(w, h),
        "leftRightUpArrow" => left_right_up_arrow(w, h),
        "bentArrow" => poly(&[
            (0.0, h * 0.75),
            (w * 0.55, h * 0.75),
            (w * 0.55, h * 0.45),
            (w * 0.35, h * 0.45),
            (w * 0.7, 0.0),
            (w, h * 0.45),
            (w * 0.8, h * 0.45),
            (w * 0.8, h),
            (0.0, h),
        ]),
        "bentUpArrow" => poly(&[
            (0.0, h),
            (w * 0.65, h),
            (w * 0.65, h * 0.6),
            (w * 0.9, h * 0.6),
            (w * 0.45, 0.0),
            (w * 0.2, h * 0.6),
            (w * 0.45, h * 0.6),
            (w * 0.45, h * 0.8),
            (0.0, h * 0.8),
        ]),
        "uturnArrow" => poly(&[
            (0.0, h),
            (0.0, h * 0.35),
            (w * 0.5, 0.0),
            (w, h * 0.35),
            (w, h * 0.65),
            (w * 0.75, h * 0.65),
            (w * 0.75, h * 0.45),
            (w * 0.5, h * 0.3),
            (w * 0.25, h * 0.45),
            (w * 0.25, h),
        ]),
        "notchedRightArrow" => poly(&[
            (0.0, h * 0.5),
            (w * 0.25, 0.0),
            (w * 0.6, 0.0),
            (w * 0.6, h * 0.25),
            (w, h * 0.5),
            (w * 0.6, h * 0.75),
            (w * 0.6, h),
            (w * 0.25, h),
        ]),
        "stripedRightArrow" => poly_evenodd(&[
            &[
                (0.0, h * 0.25),
                (w * 0.6, h * 0.25),
                (w * 0.6, 0.0),
                (w, h * 0.5),
                (w * 0.6, h),
                (w * 0.6, h * 0.75),
                (0.0, h * 0.75),
            ],
            &[
                (w * 0.15, h * 0.3),
                (w * 0.25, h * 0.3),
                (w * 0.25, h * 0.7),
                (w * 0.15, h * 0.7),
            ],
        ]),
        "chevron" => poly(&[
            (0.0, 0.0),
            (w * 0.75, 0.0),
            (w, h * 0.5),
            (w * 0.75, h),
            (0.0, h),
            (w * 0.25, h * 0.5),
        ]),
        "homePlate" => poly(&[
            (0.0, 0.0),
            (w * 0.75, 0.0),
            (w, h * 0.5),
            (w * 0.75, h),
            (0.0, h),
        ]),
        "swooshArrow" => {
            let mut b = P::default();
            b.m(w * 0.05, h * 0.55)
                .c(w * 0.05, h * 0.2, w * 0.45, 0.0, w * 0.7, h * 0.05)
                .l(w * 0.75, h * 0.25)
                .c(w * 0.55, h * 0.3, w * 0.35, h * 0.3, w * 0.2, h * 0.45)
                .l(w * 0.3, h * 0.55)
                .z()
                .finish()
        }
        "curvedDownArrow" => quarter_arrow(w, h, 90.0),
        "curvedUpArrow" => quarter_arrow(w, h, -90.0),
        "curvedLeftArrow" => quarter_arrow(w, h, 180.0),
        "curvedRightArrow" => quarter_arrow(w, h, 0.0),
        "circularArrow" => circular_arrow(w, h, 0.0, 300.0, false),
        "leftCircularArrow" => circular_arrow(w, h, 0.0, 300.0, true),
        "leftRightCircularArrow" => circular_arrow(w, h, 0.0, 360.0, false),

        "can" => cylinder(w, h, 0.18),
        "cube" => cube(w, h),
        "bevel" => plaque(0.0, 0.0, w, h, min * 0.16),
        "donut" => poly_evenodd(&[
            &ellipse_points(w / 2.0, h / 2.0, w / 2.0, h / 2.0),
            &ellipse_points(w / 2.0, h / 2.0, w / 2.0 * 0.55, h / 2.0 * 0.55),
        ]),
        "noSmoking" => format!(
            "{} {}",
            ellipse(w / 2.0, h / 2.0, w / 2.0, h / 2.0),
            p(&[
                (w * 0.15, h * 0.32),
                (w * 0.32, h * 0.15),
                (w * 0.85, h * 0.68),
                (w * 0.68, h * 0.85),
            ])
        ),
        "blockArc" => block_arc(w, h, thickness, angle),
        "pie" | "pieWedge" => pie(w, h, angle),
        "chord" => chord(w, h, angle),
        "arc" => arc(w, h, angle),
        "moon" => moon(w, h),
        "sun" => sun(w, h),
        "smileyFace" => smiley(w, h),
        "teardrop" => teardrop(w, h),
        "heart" => heart(w, h),
        "lightningBolt" => poly(&[
            (w * 0.45, 0.0),
            (w * 0.95, 0.0),
            (w * 0.6, h * 0.38),
            (w * 0.85, h * 0.38),
            (w * 0.25, h),
            (w * 0.45, h * 0.58),
            (w * 0.15, h * 0.58),
        ]),
        "cloud" => cloud(w, h),
        "cloudCallout" => format!("{} {}", cloud(w, h * 0.8), callout_tail(w, h, 0.2, 0.7)),
        "funnel" => poly(&[
            (0.0, 0.0),
            (w, 0.0),
            (w * 0.62, h * 0.55),
            (w * 0.62, h),
            (w * 0.38, h),
            (w * 0.38, h * 0.55),
        ]),
        "gear6" => gear(w, h, 6),
        "gear9" => gear(w, h, 9),
        "folderCorner" => poly(&[
            (0.0, h * 0.15),
            (w * 0.4, h * 0.15),
            (w * 0.5, h * 0.3),
            (w, h * 0.3),
            (w, h * 0.85),
            (w * 0.85, h),
            (0.0, h),
        ]),
        "irregularSeal1" | "irregularSeal2" => irregular_seal(w, h, id.ends_with('2')),
        "chartPlus" => poly_evenodd(&[
            &[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)],
            &plus_outline(w * 0.5, h * 0.5, w * 0.5, h * 0.5),
        ]),
        "chartStar" => poly_evenodd(&[
            &[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)],
            &star_points(w / 2.0, h / 2.0, w * 0.4, h * 0.4, 5, 0.4),
        ]),
        "chartX" => poly_evenodd(&[
            &[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)],
            &[
                (w * 0.2, h * 0.2),
                (w * 0.35, h * 0.2),
                (w * 0.8, h * 0.65),
                (w * 0.65, h * 0.8),
            ],
            &[
                (w * 0.65, h * 0.2),
                (w * 0.8, h * 0.2),
                (w * 0.8, h * 0.35),
                (w * 0.35, h * 0.8),
                (w * 0.2, h * 0.8),
                (w * 0.2, h * 0.65),
            ],
        ]),

        "ribbon" => ribbon(w, h, false),
        "ribbon2" => ribbon(w, h, true),
        "ellipseRibbon" => ellipse_ribbon(w, h, false),
        "ellipseRibbon2" => ellipse_ribbon(w, h, true),
        "leftRightRibbon" => format!(
            "{} {}",
            ribbon(w, h * 0.7, false),
            callout_tail_flip(w, h, 0.2, 0.3)
        ),
        "doubleWave" => wave_band(w, h, true),
        "wave" => wave_band(w, h, false),
        "verticalScroll" => scroll(w, h, true),
        "horizontalScroll" => scroll(w, h, false),
        "leftBracket" => left_bracket(w, h),
        "rightBracket" => mirror_h(&left_bracket(w, h), w),
        "leftBrace" => left_brace(w, h),
        "rightBrace" => mirror_h(&left_brace(w, h), w),
        "bracketPair" => format!(
            "{} {}",
            left_bracket(w, h),
            mirror_h(&left_bracket(w, h), w)
        ),
        "bracePair" => format!("{} {}", left_brace(w, h), mirror_h(&left_brace(w, h), w)),
        "line" => format!("M 0 0 L {} {}", f(w), f(h)),
        "lineInv" => format!("M 0 {} L {} 0", f(h), f(w)),
        "custGeom" => cust_geom(w, h, opts)?,

        id if id.starts_with("flowChart") => flow_chart(id, w, h),
        id if id.starts_with("actionButton") => action_button(w, h),
        id if id.starts_with("leftArrowCallout")
            || id.starts_with("rightArrowCallout")
            || id.starts_with("upArrowCallout")
            || id.starts_with("downArrowCallout")
            || id.starts_with("leftRightArrowCallout")
            || id.starts_with("upDownArrowCallout")
            || id.starts_with("quadArrowCallout") =>
        {
            arrow_callout(id, w, h)
        }
        id if id.contains("Callout") || id.starts_with("callout") => callout(id, w, h, r),
        _ => return None,
    };
    Some(path)
}

/// Shapes whose path needs `fill-rule="evenodd"` (they contain holes).
pub fn uses_evenodd(id: &str) -> bool {
    matches!(
        id,
        "frame"
            | "halfFrame"
            | "corner"
            | "diagStripe"
            | "stripedRightArrow"
            | "donut"
            | "chartPlus"
            | "chartStar"
            | "chartX"
            | "smileyFace"
    )
}

/// Shapes that are open strokes and never have a fill.
pub fn is_open_line(id: &str) -> bool {
    matches!(id, "line" | "lineInv" | "arc")
}

fn f(v: f64) -> String {
    let rounded = (v * 10000.0).round() / 10000.0;
    if rounded == 0.0 {
        "0".to_string()
    } else {
        format!("{rounded}")
    }
}

/// A tiny SVG path builder: fewer arguments to miscount than `format!`.
#[derive(Default)]
struct P(String);

impl P {
    fn m(&mut self, x: f64, y: f64) -> &mut Self {
        self.0.push_str(&format!(" M {} {}", f(x), f(y)));
        self
    }
    fn l(&mut self, x: f64, y: f64) -> &mut Self {
        self.0.push_str(&format!(" L {} {}", f(x), f(y)));
        self
    }
    fn c(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, x: f64, y: f64) -> &mut Self {
        self.0.push_str(&format!(
            " C {} {} {} {} {} {}",
            f(x1),
            f(y1),
            f(x2),
            f(y2),
            f(x),
            f(y)
        ));
        self
    }
    fn a(
        &mut self,
        rx: f64,
        ry: f64,
        rotation: f64,
        large: u8,
        sweep: u8,
        end: (f64, f64),
    ) -> &mut Self {
        let (x, y) = end;
        self.0.push_str(&format!(
            " A {} {} {} {} {} {} {}",
            f(rx),
            f(ry),
            f(rotation),
            large,
            sweep,
            f(x),
            f(y)
        ));
        self
    }
    fn z(&mut self) -> &mut Self {
        self.0.push_str(" Z");
        self
    }
    fn finish(&self) -> String {
        self.0.trim().to_string()
    }
}

fn pt(x: f64, y: f64) -> String {
    format!("{} {}", f(x), f(y))
}

fn poly(points: &[(f64, f64)]) -> String {
    let mut out = String::new();
    for (i, (x, y)) in points.iter().enumerate() {
        out.push_str(if i == 0 { "M " } else { " L " });
        out.push_str(&pt(*x, *y));
    }
    out.push_str(" Z");
    out
}

fn poly_evenodd(subpaths: &[&[(f64, f64)]]) -> String {
    subpaths
        .iter()
        .map(|points| poly(points))
        .collect::<Vec<_>>()
        .join(" ")
}

fn polar(cx: f64, cy: f64, rx: f64, ry: f64, deg: f64) -> (f64, f64) {
    let a = deg * PI / 180.0;
    (cx + rx * a.cos(), cy + ry * a.sin())
}

fn regular(cx: f64, cy: f64, rx: f64, ry: f64, n: usize, start: f64) -> String {
    let points: Vec<(f64, f64)> = (0..n)
        .map(|i| polar(cx, cy, rx, ry, start + 360.0 * i as f64 / n as f64))
        .collect();
    poly(&points)
}

fn star_points(cx: f64, cy: f64, rx: f64, ry: f64, n: usize, inner: f64) -> Vec<(f64, f64)> {
    let mut points = Vec::with_capacity(n * 2);
    for i in 0..n * 2 {
        let deg = -90.0 + 360.0 * i as f64 / (n * 2) as f64;
        let (x, y) = if i % 2 == 0 {
            polar(cx, cy, rx, ry, deg)
        } else {
            polar(cx, cy, rx * inner, ry * inner, deg)
        };
        points.push((x, y));
    }
    points
}

fn star(cx: f64, cy: f64, rx: f64, ry: f64, n: usize, inner: f64) -> String {
    poly(&star_points(cx, cy, rx, ry, n, inner))
}

fn ellipse_points(cx: f64, cy: f64, rx: f64, ry: f64) -> Vec<(f64, f64)> {
    let mut points = Vec::with_capacity(64);
    for i in 0..64 {
        points.push(polar(cx, cy, rx, ry, 360.0 * i as f64 / 64.0));
    }
    points
}

fn ellipse(cx: f64, cy: f64, rx: f64, ry: f64) -> String {
    format!(
        "M {} A {} {} 0 1 0 {} A {} {} 0 1 0 {} Z",
        pt(cx - rx, cy),
        f(rx),
        f(ry),
        pt(cx + rx, cy),
        f(rx),
        f(ry),
        pt(cx - rx, cy)
    )
}

fn round_rect(x: f64, y: f64, w: f64, h: f64, radii: [f64; 4]) -> String {
    let max = w.min(h) / 2.0;
    let [tl, tr, br, bl] = radii.map(|r| r.clamp(0.0, max));
    let mut s = String::new();
    s.push_str(&format!("M {} ", pt(x + tl, y)));
    s.push_str(&format!("L {} ", pt(x + w - tr, y)));
    if tr > 0.0 {
        s.push_str(&format!(
            "A {} {} 0 0 1 {} ",
            f(tr),
            f(tr),
            pt(x + w, y + tr)
        ));
    }
    s.push_str(&format!("L {} ", pt(x + w, y + h - br)));
    if br > 0.0 {
        s.push_str(&format!(
            "A {} {} 0 0 1 {} ",
            f(br),
            f(br),
            pt(x + w - br, y + h)
        ));
    }
    s.push_str(&format!("L {} ", pt(x + bl, y + h)));
    if bl > 0.0 {
        s.push_str(&format!(
            "A {} {} 0 0 1 {} ",
            f(bl),
            f(bl),
            pt(x, y + h - bl)
        ));
    }
    s.push_str(&format!("L {} ", pt(x, y + tl)));
    if tl > 0.0 {
        s.push_str(&format!("A {} {} 0 0 1 {} ", f(tl), f(tl), pt(x + tl, y)));
    }
    s.push('Z');
    s
}

fn snip_rect(x: f64, y: f64, w: f64, h: f64, cuts: [f64; 4], which: [f64; 4]) -> String {
    // which: [tl, tr, br, bl] 1.0 means snip that corner.
    let c = |i: usize| cuts[i] * which[i];
    poly(&[
        (x + c(0), y),
        (x + w - c(1), y),
        (x + w, y + c(1)),
        (x + w, y + h - c(2)),
        (x + w - c(2), y + h),
        (x + c(3), y + h),
        (x, y + h - c(3)),
        (x, y + c(0)),
    ])
}

fn plaque(x: f64, y: f64, w: f64, h: f64, cut: f64) -> String {
    let c = cut.min(w / 2.0).min(h / 2.0);
    poly(&[
        (x + c, y),
        (x + w - c, y),
        (x + w, y + c),
        (x + w, y + h - c),
        (x + w - c, y + h),
        (x + c, y + h),
        (x, y + h - c),
        (x, y + c),
    ])
}

fn frame(x: f64, y: f64, w: f64, h: f64, t: f64) -> String {
    let iw = w * t;
    let ih = h * t;
    poly_evenodd(&[
        &[(x, y), (x + w, y), (x + w, y + h), (x, y + h)],
        &[
            (x + iw, y + ih),
            (x + iw, y + h - ih),
            (x + w - iw, y + h - ih),
            (x + w - iw, y + ih),
        ],
    ])
}

fn plus(w: f64, h: f64, t: f64) -> String {
    let tx = w * t;
    let ty = h * t;
    poly(&[
        (tx, 0.0),
        (w - tx, 0.0),
        (w - tx, ty),
        (w, ty),
        (w, h - ty),
        (w - tx, h - ty),
        (w - tx, h),
        (tx, h),
        (tx, h - ty),
        (0.0, h - ty),
        (0.0, ty),
        (tx, ty),
    ])
}

fn plus_outline(cx: f64, cy: f64, w: f64, h: f64) -> Vec<(f64, f64)> {
    let tx = w * 0.5;
    let ty = h * 0.5;
    vec![
        (cx - tx, cy - ty * 0.4),
        (cx - tx * 0.4, cy - ty * 0.4),
        (cx - tx * 0.4, cy - ty),
        (cx + tx * 0.4, cy - ty),
        (cx + tx * 0.4, cy - ty * 0.4),
        (cx + tx, cy - ty * 0.4),
        (cx + tx, cy + ty * 0.4),
        (cx + tx * 0.4, cy + ty * 0.4),
        (cx + tx * 0.4, cy + ty),
        (cx - tx * 0.4, cy + ty),
        (cx - tx * 0.4, cy + ty * 0.4),
        (cx - tx, cy + ty * 0.4),
    ]
}

fn arrow_right(w: f64, h: f64) -> String {
    poly(&[
        (0.0, h * 0.25),
        (w * 0.62, h * 0.25),
        (w * 0.62, 0.0),
        (w, h * 0.5),
        (w * 0.62, h),
        (w * 0.62, h * 0.75),
        (0.0, h * 0.75),
    ])
}

fn left_right_arrow(w: f64, h: f64) -> String {
    poly(&[
        (0.0, h * 0.5),
        (w * 0.2, 0.0),
        (w * 0.2, h * 0.25),
        (w * 0.8, h * 0.25),
        (w * 0.8, 0.0),
        (w, h * 0.5),
        (w * 0.8, h),
        (w * 0.8, h * 0.75),
        (w * 0.2, h * 0.75),
        (w * 0.2, h),
    ])
}

fn quad_arrow(w: f64, h: f64) -> String {
    poly(&[
        (w * 0.35, 0.0),
        (w * 0.65, 0.0),
        (w * 0.65, h * 0.35),
        (w, h * 0.35),
        (w, h * 0.65),
        (w * 0.65, h * 0.65),
        (w * 0.65, h),
        (w * 0.35, h),
        (w * 0.35, h * 0.65),
        (0.0, h * 0.65),
        (0.0, h * 0.35),
        (w * 0.35, h * 0.35),
    ])
}

fn left_up_arrow(w: f64, h: f64) -> String {
    poly(&[
        (0.0, h * 0.5),
        (w * 0.35, 0.0),
        (w * 0.35, h * 0.25),
        (w * 0.65, h * 0.25),
        (w * 0.65, h * 0.45),
        (w * 0.45, h * 0.45),
        (w * 0.45, h * 0.8),
        (w * 0.65, h * 0.8),
        (w * 0.65, h),
        (w * 0.25, h),
        (w * 0.25, h * 0.6),
        (0.0, h * 0.6),
    ])
}

fn left_right_up_arrow(w: f64, h: f64) -> String {
    poly(&[
        (0.0, h * 0.55),
        (w * 0.2, h * 0.2),
        (w * 0.2, h * 0.35),
        (w * 0.4, h * 0.35),
        (w * 0.4, 0.0),
        (w * 0.6, 0.0),
        (w * 0.6, h * 0.35),
        (w * 0.8, h * 0.35),
        (w * 0.8, h * 0.2),
        (w, h * 0.55),
        (w * 0.8, h * 0.9),
        (w * 0.8, h * 0.75),
        (w * 0.6, h * 0.75),
        (w * 0.6, h),
        (w * 0.4, h),
        (w * 0.4, h * 0.75),
        (w * 0.2, h * 0.75),
        (w * 0.2, h * 0.9),
    ])
}

fn quarter_arrow(w: f64, h: f64, rotation: f64) -> String {
    let body = format!(
        "M {} {} A {} {} 0 0 1 {} {} L {} {} L {} {} A {} {} 0 0 0 {} {} L {} {} Z",
        f(w * 0.1),
        f(h * 0.9),
        f(w * 0.8),
        f(h * 0.8),
        f(w * 0.9),
        f(h * 0.1),
        f(w * 0.55),
        f(h * 0.1),
        f(w * 0.55),
        f(h * 0.25),
        f(w * 0.6),
        f(h * 0.6),
        f(w * 0.25),
        f(h * 0.55),
        f(w * 0.25),
        f(h * 0.9)
    );
    if rotation == 0.0 {
        body
    } else {
        rotate_path(&body, w, h, rotation)
    }
}

fn circular_arrow(w: f64, h: f64, start: f64, sweep: f64, reverse: bool) -> String {
    let cx = w / 2.0;
    let cy = h / 2.0;
    let ro_x = w * 0.42;
    let ro_y = h * 0.42;
    let ri_x = w * 0.24;
    let ri_y = h * 0.24;
    let s = if reverse { start + sweep } else { start };
    let e = if reverse { start } else { start + sweep };
    let large = u8::from((e - s).abs() > 180.0);
    let sweep_flag = u8::from(e > s);
    let back = 1 - sweep_flag;
    let (ox1, oy1) = polar(cx, cy, ro_x, ro_y, s);
    let (ox2, oy2) = polar(cx, cy, ro_x, ro_y, e);
    let (ix2, iy2) = polar(cx, cy, ri_x, ri_y, e - 10.0);
    let (ix1, iy1) = polar(cx, cy, ri_x, ri_y, s + 10.0);
    let (tx, ty) = polar(cx, cy, ro_x * 1.18, ro_y * 1.18, e);
    let (hx1, hy1) = polar(cx, cy, ro_x * 1.1, ro_y * 1.1, e - 14.0);
    let (hx2, hy2) = polar(cx, cy, ro_x * 1.1, ro_y * 1.1, e + 14.0);
    let mut b = P::default();
    b.m(ox1, oy1)
        .a(ro_x, ro_y, 0.0, large, sweep_flag, (ox2, oy2))
        .l(hx1, hy1)
        .l(tx, ty)
        .l(hx2, hy2)
        .l(ix2, iy2)
        .a(ri_x, ri_y, 0.0, large, back, (ix1, iy1))
        .z()
        .finish()
}

fn block_arc(w: f64, h: f64, thickness: f64, angle: (f64, f64)) -> String {
    let cx = w / 2.0;
    let cy = h / 2.0;
    let ro_x = w / 2.0;
    let ro_y = h / 2.0;
    let ri_x = ro_x * (1.0 - thickness);
    let ri_y = ro_y * (1.0 - thickness);
    let (s, e) = angle;
    let large = if (e - s).abs() > 180.0 { 1 } else { 0 };
    let (ox1, oy1) = polar(cx, cy, ro_x, ro_y, s);
    let (ox2, oy2) = polar(cx, cy, ro_x, ro_y, e);
    let (ix1, iy1) = polar(cx, cy, ri_x, ri_y, e);
    let (ix2, iy2) = polar(cx, cy, ri_x, ri_y, s);
    let sweep_flag = if e > s { 1 } else { 0 };
    let back = if e > s { 0 } else { 1 };
    format!(
        "M {} A {} {} 0 {} {} {} L {} A {} {} 0 {} {} {} Z",
        pt(ox1, oy1),
        f(ro_x),
        f(ro_y),
        large,
        sweep_flag,
        pt(ox2, oy2),
        pt(ix1, iy1),
        f(ri_x),
        f(ri_y),
        large,
        back,
        pt(ix2, iy2)
    )
}

fn pie(w: f64, h: f64, angle: (f64, f64)) -> String {
    let cx = w / 2.0;
    let cy = h / 2.0;
    let (s, e) = angle;
    let large = if (e - s).abs() > 180.0 { 1 } else { 0 };
    let (x1, y1) = polar(cx, cy, cx, cy, s);
    let (x2, y2) = polar(cx, cy, cx, cy, e);
    let sweep_flag = if e > s { 1 } else { 0 };
    format!(
        "M {} L {} A {} {} 0 {} {} {} Z",
        pt(cx, cy),
        pt(x1, y1),
        f(cx),
        f(cy),
        large,
        sweep_flag,
        pt(x2, y2)
    )
}

fn chord(w: f64, h: f64, angle: (f64, f64)) -> String {
    let cx = w / 2.0;
    let cy = h / 2.0;
    let (s, e) = angle;
    let large = if (e - s).abs() > 180.0 { 1 } else { 0 };
    let (x1, y1) = polar(cx, cy, cx, cy, s);
    let (x2, y2) = polar(cx, cy, cx, cy, e);
    let sweep_flag = if e > s { 1 } else { 0 };
    format!(
        "M {} A {} {} 0 {} {} {} Z",
        pt(x1, y1),
        f(cx),
        f(cy),
        large,
        sweep_flag,
        pt(x2, y2)
    )
}

fn arc(w: f64, h: f64, angle: (f64, f64)) -> String {
    let cx = w / 2.0;
    let cy = h / 2.0;
    let (s, e) = angle;
    let large = if (e - s).abs() > 180.0 { 1 } else { 0 };
    let (x1, y1) = polar(cx, cy, cx, cy, s);
    let (x2, y2) = polar(cx, cy, cx, cy, e);
    let sweep_flag = if e > s { 1 } else { 0 };
    format!(
        "M {} A {} {} 0 {} {} {}",
        pt(x1, y1),
        f(cx),
        f(cy),
        large,
        sweep_flag,
        pt(x2, y2)
    )
}

fn cylinder(w: f64, h: f64, lid: f64) -> String {
    let rx = w / 2.0;
    let ry = (h * lid / 2.0).max(0.01);
    let bottom = h - ry;
    format!(
        "M 0 {} L 0 {} A {} {} 0 0 1 {} {} L {} {} A {} {} 0 0 0 0 {} Z {}",
        f(ry),
        f(bottom),
        f(rx),
        f(ry),
        f(w),
        f(bottom),
        f(w),
        f(ry),
        f(rx),
        f(ry),
        f(ry),
        ellipse(rx, ry, rx, ry)
    )
}

fn cube(w: f64, h: f64) -> String {
    let d = w.min(h) * 0.22;
    format!(
        "{} {} {}",
        poly(&[
            (0.0, d),
            (w - d, d),
            (w, 0.0),
            (w - d, 0.0),
            (0.0, 0.0),
            (d, d),
            (d, h),
            (0.0, h - d),
        ]),
        poly(&[
            (w - d, d),
            (w, 0.0),
            (w, h - d),
            (w - d, h),
            (d, h),
            (0.0, h - d),
            (w - d, h - d),
        ]),
        poly(&[(w - d, d), (w, 0.0), (w, h - d), (w - d, h)])
    )
}

fn moon(w: f64, h: f64) -> String {
    let mut b = P::default();
    b.m(w * 0.55, 0.0)
        .c(w * 0.85, 0.0, w, h * 0.2, w, h * 0.5)
        .c(w, h * 0.8, w * 0.85, h, w * 0.55, h)
        .c(w * 0.75, h * 0.7, w * 0.75, h * 0.3, w * 0.55, 0.0)
        .z()
        .finish()
}

fn sun(w: f64, h: f64) -> String {
    let cx = w / 2.0;
    let cy = h / 2.0;
    let r = w.min(h) * 0.28;
    let mut out = ellipse(cx, cy, r, r);
    for i in 0..8 {
        let a = 360.0 * i as f64 / 8.0;
        let (x1, y1) = polar(cx, cy, r * 1.2, r * 1.2, a - 8.0);
        let (x2, y2) = polar(cx, cy, r * 1.8, r * 1.8, a);
        let (x3, y3) = polar(cx, cy, r * 1.2, r * 1.2, a + 8.0);
        out.push(' ');
        out.push_str(&poly(&[(x1, y1), (x2, y2), (x3, y3)]));
    }
    out
}

fn smiley(w: f64, h: f64) -> String {
    let cx = w / 2.0;
    let cy = h / 2.0;
    let r = w.min(h) / 2.0;
    poly_evenodd(&[
        &ellipse_points(cx, cy, r, r),
        &ellipse_points(cx - r * 0.35, cy - r * 0.2, r * 0.1, r * 0.12),
        &ellipse_points(cx + r * 0.35, cy - r * 0.2, r * 0.1, r * 0.12),
        &[
            (cx - r * 0.45, cy + r * 0.15),
            (cx + r * 0.45, cy + r * 0.15),
            (cx + r * 0.3, cy + r * 0.55),
            (cx - r * 0.3, cy + r * 0.55),
        ],
    ])
}

fn teardrop(w: f64, h: f64) -> String {
    let mut b = P::default();
    b.m(w * 0.5, 0.0)
        .c(w * 0.75, h * 0.15, w, h * 0.55, w * 0.85, h * 0.78)
        .c(w * 0.72, h * 0.97, w * 0.28, h * 0.97, w * 0.15, h * 0.78)
        .c(0.0, h * 0.55, w * 0.25, h * 0.15, w * 0.5, 0.0)
        .z()
        .finish()
}

fn heart(w: f64, h: f64) -> String {
    let mut b = P::default();
    b.m(w * 0.5, h)
        .c(w * 0.15, h * 0.8, 0.0, h * 0.45, w * 0.22, h * 0.2)
        .c(w * 0.35, h * 0.02, w * 0.45, h * 0.05, w * 0.5, h * 0.2)
        .c(w * 0.55, h * 0.05, w * 0.65, h * 0.02, w * 0.78, h * 0.2)
        .c(w, h * 0.45, w * 0.85, h * 0.8, w * 0.5, h)
        .z()
        .finish()
}

fn cloud(w: f64, h: f64) -> String {
    let mut b = P::default();
    b.m(w * 0.2, h * 0.75)
        .c(0.0, h * 0.6, w * 0.05, h * 0.3, w * 0.25, h * 0.25)
        .c(w * 0.35, h * 0.05, w * 0.65, 0.0, w * 0.75, h * 0.2)
        .c(w * 0.95, h * 0.15, w, h * 0.4, w * 0.9, h * 0.6)
        .c(w * 0.95, h * 0.8, w * 0.75, h * 0.9, w * 0.6, h * 0.9)
        .c(w * 0.4, h, w * 0.25, h * 0.9, w * 0.2, h * 0.75)
        .z()
        .finish()
}

fn gear(w: f64, h: f64, teeth: usize) -> String {
    let cx = w / 2.0;
    let cy = h / 2.0;
    let ro = w.min(h) / 2.0;
    let ri = ro * 0.72;
    let mut points = Vec::new();
    for i in 0..teeth * 4 {
        let deg = 360.0 * i as f64 / (teeth * 4) as f64;
        let r = match i % 4 {
            0 | 3 => ro,
            _ => ri,
        };
        points.push(polar(cx, cy, r, r, deg));
    }
    poly(&points)
}

fn irregular_seal(w: f64, h: f64, second: bool) -> String {
    let cx = w / 2.0;
    let cy = h / 2.0;
    let mut points = Vec::new();
    let n = 16;
    for i in 0..n {
        let deg = 360.0 * i as f64 / n as f64;
        let jitter = if second {
            1.0 - 0.18 * ((i * 7 % 5) as f64 / 5.0)
        } else {
            1.0 - 0.22 * ((i * 3 % 7) as f64 / 7.0)
        };
        points.push(polar(cx, cy, w / 2.0 * jitter, h / 2.0 * jitter, deg));
    }
    poly(&points)
}

fn ribbon(w: f64, h: f64, flip: bool) -> String {
    let band = h * 0.55;
    let y0 = if flip { h - band } else { 0.0 };
    let y1 = y0 + band;
    let tail = h * 0.2;
    let notch = w * 0.12;
    poly(&[
        (0.0, y0 + tail),
        (notch, y0),
        (w - notch, y0),
        (w, y0 + tail),
        (w - notch * 0.6, y0 + tail * 0.6),
        (w - notch * 0.6, y1),
        (notch * 0.6, y1),
        (notch * 0.6, y0 + tail * 0.6),
    ])
}

fn ellipse_ribbon(w: f64, h: f64, flip: bool) -> String {
    let cy = if flip { h * 0.7 } else { h * 0.3 };
    let rx = w * 0.45;
    let ry = h * 0.28;
    let band = format!(
        "{} {}",
        ellipse(w / 2.0, cy, rx, ry),
        poly(&[
            (w * 0.1, cy - ry * 0.2),
            (0.0, cy - ry * 1.2),
            (w * 0.2, cy - ry * 1.35),
            (w * 0.3, cy - ry * 0.3),
        ])
    );
    let band2 = poly(&[
        (w * 0.9, cy - ry * 0.2),
        (w, cy - ry * 1.2),
        (w * 0.8, cy - ry * 1.35),
        (w * 0.7, cy - ry * 0.3),
    ]);
    format!("{band} {band2}")
}

fn wave_band(w: f64, h: f64, double: bool) -> String {
    let amp = if double { h * 0.18 } else { h * 0.14 };
    let y_top = h * 0.5;
    let y_bot = if double { h * 0.65 } else { h * 0.55 };
    let steps = 4;
    let mut b = P::default();
    b.m(0.0, y_top);
    for i in 0..steps {
        let x0 = w * i as f64 / steps as f64;
        let x1 = w * (i + 1) as f64 / steps as f64;
        let dir = if i % 2 == 0 { -1.0 } else { 1.0 };
        let dx = (x1 - x0) / 3.0;
        b.c(
            x0 + dx,
            y_top + dir * amp,
            x0 + 2.0 * dx,
            y_top + dir * amp,
            x1,
            y_top,
        );
    }
    b.l(w, y_bot);
    for i in (0..steps).rev() {
        let x0 = w * i as f64 / steps as f64;
        let x1 = w * (i + 1) as f64 / steps as f64;
        let dir = if i % 2 == 0 { -1.0 } else { 1.0 };
        let dx = (x1 - x0) / 3.0;
        b.c(
            x1 - dx,
            y_bot - dir * amp,
            x0 + dx,
            y_bot - dir * amp,
            x0,
            y_bot,
        );
    }
    b.z().finish()
}

fn scroll(w: f64, h: f64, vertical: bool) -> String {
    let mut b = P::default();
    if vertical {
        b.m(w * 0.1, h * 0.1)
            .c(w * 0.1, 0.0, w * 0.9, 0.0, w * 0.9, h * 0.1)
            .l(w * 0.9, h * 0.9)
            .c(w * 0.9, h, w * 0.1, h, w * 0.1, h * 0.9)
            .z();
    } else {
        b.m(w * 0.1, h * 0.1)
            .c(0.0, h * 0.1, 0.0, h * 0.9, w * 0.1, h * 0.9)
            .l(w * 0.9, h * 0.9)
            .c(w, h * 0.9, w, h * 0.1, w * 0.9, h * 0.1)
            .z();
    }
    b.finish()
}

fn left_bracket(w: f64, h: f64) -> String {
    let t = w * 0.3;
    poly(&[
        (w, 0.0),
        (0.0, 0.0),
        (0.0, h),
        (w, h),
        (w, h - t),
        (t, h - t),
        (t, t),
        (w, t),
    ])
}

fn left_brace(w: f64, h: f64) -> String {
    let t = w * 0.22;
    poly(&[
        (w, 0.0),
        (w * 0.55, 0.0),
        (w * 0.55, h * 0.4),
        (w * 0.2, h * 0.5),
        (w * 0.55, h * 0.6),
        (w * 0.55, h),
        (w, h),
        (w, h - t),
        (w * 0.7, h - t),
        (w * 0.7, h * 0.62),
        (w * 0.45, h * 0.52),
        (w * 0.7, h * 0.38),
        (w * 0.7, t),
        (w, t),
    ])
}

fn callout_tail(w: f64, h: f64, from: f64, to: f64) -> String {
    poly(&[
        (w * from, h * 0.7),
        (w * to, h),
        (w * (from + 0.15), h * 0.7),
    ])
}

fn callout_tail_flip(w: f64, h: f64, from: f64, to: f64) -> String {
    poly(&[
        (w * from, h * 0.3),
        (w * to, 0.0),
        (w * (from + 0.15), h * 0.3),
    ])
}

fn callout(id: &str, w: f64, h: f64, r: f64) -> String {
    let body_h = h * 0.78;
    let body = if id.contains("Ellipse") {
        ellipse(w / 2.0, body_h / 2.0, w / 2.0, body_h / 2.0)
    } else if id.contains("RoundRect") {
        round_rect(0.0, 0.0, w, body_h, [r, r, r, r])
    } else {
        poly(&[(0.0, 0.0), (w, 0.0), (w, body_h), (0.0, body_h)])
    };
    let tails: Vec<String> = match id {
        "callout1" | "borderCallout1" | "accentCallout1" | "accentBorderCallout1" => {
            vec![callout_tail(w, h, 0.2, 0.32)]
        }
        "callout2" | "borderCallout2" | "accentCallout2" | "accentBorderCallout2" => {
            vec![callout_tail(w, h, 0.2, 0.3), callout_tail(w, h, 0.62, 0.5)]
        }
        "callout3" | "borderCallout3" | "accentCallout3" | "accentBorderCallout3" => vec![
            callout_tail(w, h, 0.15, 0.22),
            callout_tail(w, h, 0.5, 0.62),
            callout_tail(w, h, 0.8, 0.95),
        ],
        _ => vec![callout_tail(w, h, 0.22, 0.35)],
    };
    format!("{body} {}", tails.join(" "))
}

fn arrow_callout(id: &str, w: f64, h: f64) -> String {
    let (body, tail_from) = if id.starts_with("left") && !id.starts_with("leftRight") {
        (mirror_h(&arrow_right(w, h), w), 0.2)
    } else if id.starts_with("right") {
        (arrow_right(w, h), 0.2)
    } else if id.starts_with("up") {
        (rotate90(&arrow_right(h, w), w, h), 0.5)
    } else if id.starts_with("down") {
        (rotate270(&arrow_right(h, w), w, h), 0.5)
    } else if id.starts_with("leftRight") {
        (left_right_arrow(w, h), 0.5)
    } else if id.starts_with("upDown") {
        (rotate90(&left_right_arrow(h, w), w, h), 0.5)
    } else {
        (quad_arrow(w, h), 0.5)
    };
    let tail = poly(&[
        (w * tail_from, h * 0.2),
        (w * (tail_from + 0.1), h * 0.05),
        (w * (tail_from + 0.18), h * 0.2),
    ]);
    format!("{body} {tail}")
}

fn flow_chart(id: &str, w: f64, h: f64) -> String {
    let short = id.trim_start_matches("flowChart");
    match short {
        "AlternateProcess" => round_rect(0.0, 0.0, w, h, [h * 0.2, h * 0.2, h * 0.2, h * 0.2]),
        "Process" | "PredefinedProcess" => poly(&[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]),
        "Decision" => poly(&[(w / 2.0, 0.0), (w, h / 2.0), (w / 2.0, h), (0.0, h / 2.0)]),
        "Data" | "InputOutput" => poly(&[(w * 0.2, 0.0), (w, 0.0), (w * 0.8, h), (0.0, h)]),
        "Document" => {
            let mut b = P::default();
            b.m(0.0, 0.0)
                .l(w, 0.0)
                .l(w, h * 0.85)
                .c(w * 0.7, h, w * 0.5, h * 0.82, w * 0.3, h * 0.65)
                .c(w * 0.15, h * 0.92, w * 0.15, h * 0.85, 0.0, h * 0.85)
                .z()
                .finish()
        }
        "Multidocument" => format!(
            "{} {}",
            poly(&[(0.0, h * 0.12), (w * 0.8, h * 0.12), (w * 0.8, h)]),
            poly(&[
                (w * 0.15, 0.0),
                (w, 0.0),
                (w, h * 0.88),
                (w * 0.15, h * 0.88)
            ])
        ),
        "InternalStorage" => format!(
            "{} M {} 0 L {} {} M 0 {} L {} {}",
            poly(&[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]),
            f(w * 0.2),
            f(w * 0.2),
            f(h),
            f(h * 0.2),
            f(w),
            f(h * 0.2)
        ),
        "ManualInput" => poly(&[(0.0, h * 0.2), (w, 0.0), (w, h), (0.0, h)]),
        "ManualOperation" => poly(&[(0.0, 0.0), (w, 0.0), (w * 0.8, h), (w * 0.2, h)]),
        "Preparation" => poly(&[
            (w * 0.2, 0.0),
            (w * 0.8, 0.0),
            (w, h / 2.0),
            (w * 0.8, h),
            (w * 0.2, h),
            (0.0, h / 2.0),
        ]),
        "Connector" => ellipse(w / 2.0, h / 2.0, w / 2.0, h / 2.0),
        "OffpageConnector" => poly(&[
            (0.0, 0.0),
            (w, 0.0),
            (w, h * 0.7),
            (w / 2.0, h),
            (0.0, h * 0.7),
        ]),
        "PunchedCard" => {
            let c = w.min(h) * 0.15;
            poly(&[
                (c, 0.0),
                (w, 0.0),
                (w, h),
                (0.0, h),
                (0.0, c),
                (c, c),
                (c, h - c),
                (w - c, h - c),
                (w - c, 0.0),
            ])
        }
        "PunchedTape" => format!(
            "{} {}",
            poly(&[
                (0.0, h * 0.1),
                (w * 0.25, 0.0),
                (w * 0.75, 0.0),
                (w, h * 0.1),
                (w, h * 0.9),
                (w * 0.75, h),
                (w * 0.25, h),
                (0.0, h * 0.9),
            ]),
            ellipse(w * 0.5, h * 0.1, w * 0.25, h * 0.1)
        ),
        "SummingJunction" => format!(
            "{} M {} {} L {} {} M {} {} L {} {}",
            ellipse(w / 2.0, h / 2.0, w / 2.0, h / 2.0),
            f(w * 0.2),
            f(h * 0.2),
            f(w * 0.8),
            f(h * 0.8),
            f(w * 0.8),
            f(h * 0.2),
            f(w * 0.2),
            f(h * 0.8)
        ),
        "Or" => format!(
            "{} M {} {} L {} {} M {} {} L {} {}",
            ellipse(w / 2.0, h / 2.0, w / 2.0, h / 2.0),
            f(w / 2.0),
            f(h * 0.2),
            f(w / 2.0),
            f(h * 0.8),
            f(w * 0.2),
            f(h / 2.0),
            f(w * 0.8),
            f(h / 2.0)
        ),
        "Collate" => format!(
            "{} {}",
            poly(&[(0.0, 0.0), (w, 0.0), (0.0, h / 2.0)]),
            poly(&[(0.0, h / 2.0), (w, h), (w, h / 2.0)])
        ),
        "Sort" => format!(
            "{} M 0 {} L {} {}",
            poly(&[(w / 2.0, 0.0), (w, h / 2.0), (w / 2.0, h), (0.0, h / 2.0)]),
            f(h / 2.0),
            f(w),
            f(h / 2.0)
        ),
        "Extract" => poly(&[(0.0, h), (w / 2.0, 0.0), (w, h)]),
        "Merge" => poly(&[(0.0, 0.0), (w, 0.0), (w / 2.0, h)]),
        "Delay" => format!(
            "M 0 0 L {} 0 A {} {} 0 0 1 {} {} L 0 {} Z",
            f(w * 0.7),
            f(h / 2.0),
            f(h / 2.0),
            f(w * 0.7),
            f(h),
            f(h)
        ),
        "Display" => {
            let mut b = P::default();
            b.m(0.0, 0.0)
                .l(w * 0.8, 0.0)
                .l(w, h / 2.0)
                .c(w * 0.75, h, w * 0.35, h, 0.0, h)
                .z()
                .finish()
        }
        "MagneticDisk" => {
            let mut b = P::default();
            b.m(w * 0.15, 0.0)
                .l(w * 0.75, 0.0)
                .c(w, 0.0, w, h, w * 0.75, h)
                .l(w * 0.15, h)
                .c(0.0, h, 0.0, 0.0, w * 0.15, 0.0)
                .z();
            format!(
                "{} {}",
                b.finish(),
                ellipse(w * 0.75, h / 2.0, w * 0.25, h / 2.0)
            )
        }
        "MagneticDrum" => cylinder(w, h, 0.22),
        "MagneticTape" => format!(
            "{} {}",
            ellipse(w / 2.0, h / 2.0, w / 2.0, h / 2.0),
            ellipse(w / 2.0, h / 2.0, w * 0.25, h * 0.25)
        ),
        "DirectAccessStorage" => cylinder(w, h, 0.3),
        "SequentialAccessStorage" => format!(
            "{} {}",
            ellipse(w / 2.0, h / 2.0, w / 2.0, h / 2.0),
            poly(&[(w * 0.2, h * 0.2), (w * 0.8, h * 0.2), (w * 0.5, h * 0.8)]),
        ),
        "OnlineStorage" => poly(&[
            (0.0, h * 0.3),
            (w * 0.3, 0.0),
            (w * 0.7, 0.0),
            (w, h * 0.3),
            (w, h),
            (0.0, h),
        ]),
        "OfflineStorage" => poly(&[(w * 0.2, 0.0), (w * 0.8, 0.0), (w, h), (0.0, h)]),
        "Terminator" => round_rect(0.0, 0.0, w, h, [h / 2.0, h / 2.0, h / 2.0, h / 2.0]),
        _ => poly(&[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]),
    }
}

fn action_button(w: f64, h: f64) -> String {
    let r = w.min(h) * 0.12;
    format!(
        "{} {}",
        round_rect(0.0, 0.0, w, h, [r, r, r, r]),
        round_rect(
            w * 0.12,
            h * 0.12,
            w * 0.76,
            h * 0.76,
            [r * 0.7, r * 0.7, r * 0.7, r * 0.7]
        )
    )
}

fn cust_geom(w: f64, h: f64, opts: &GeomOpts) -> Option<String> {
    let points = opts.points.as_ref()?;
    let mut out = String::new();
    for point in points {
        if point.close {
            out.push_str(" Z");
            continue;
        }
        match &point.curve {
            Some(Curve::Arc {
                hr,
                wr,
                st_ang,
                sw_ang,
            }) => {
                let end = polar(point.x, point.y, *wr, *hr, st_ang + sw_ang);
                out.push_str(&format!(
                    " A {} {} 0 {} {} {}",
                    f(*wr),
                    f(*hr),
                    if sw_ang.abs() > 180.0 { 1 } else { 0 },
                    if *sw_ang >= 0.0 { 1 } else { 0 },
                    pt(end.0, end.1)
                ));
            }
            Some(Curve::Cubic { x1, y1, x2, y2 }) => {
                out.push_str(&format!(
                    " C {} {} {}",
                    pt(*x1, *y1),
                    pt(*x2, *y2),
                    pt(point.x, point.y)
                ));
            }
            Some(Curve::Quadratic { x1, y1 }) => {
                out.push_str(&format!(" Q {} {}", pt(*x1, *y1), pt(point.x, point.y)));
            }
            None => {
                out.push_str(if point.move_to || out.is_empty() {
                    " M "
                } else {
                    " L "
                });
                out.push_str(&pt(point.x, point.y));
            }
        }
    }
    let _ = (w, h);
    out.push_str(" Z");
    Some(out.trim().to_string())
}

// --- transforms -----------------------------------------------------------

fn mirror_h(path: &str, w: f64) -> String {
    transform_path(path, |x, _| w - x, |_, y| y)
}

fn rotate90(path: &str, w: f64, _h: f64) -> String {
    // 90 degrees counter-clockwise: (x,y) -> (w - y, x), mapping a
    // `h x w` source box into a `w x h` destination.
    transform_path(path, |_, y| w - y, |x, _| x)
}

fn rotate270(path: &str, _w: f64, h: f64) -> String {
    // 90 degrees clockwise: (x,y) -> (y, h - x).
    transform_path(path, |_, y| y, |x, _| h - x)
}

fn rotate_path(path: &str, w: f64, h: f64, deg: f64) -> String {
    let cx = w / 2.0;
    let cy = h / 2.0;
    let a = deg * PI / 180.0;
    transform_path(
        path,
        |x, y| cx + (x - cx) * a.cos() - (y - cy) * a.sin(),
        |x, y| cy + (x - cx) * a.sin() + (y - cy) * a.cos(),
    )
}

/// Apply a coordinate transform to every numeric pair in a path string.
fn transform_path(
    path: &str,
    fx: impl Fn(f64, f64) -> f64,
    fy: impl Fn(f64, f64) -> f64,
) -> String {
    let tokens: Vec<String> = path
        .split_whitespace()
        .map(|token| token.to_string())
        .collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let token = &tokens[i];
        if token
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
        {
            out.push(token.clone());
            // Arc flags and radii are not positions; keep the next few tokens.
            match token.as_str() {
                "A" | "a" => {
                    for _ in 0..5 {
                        i += 1;
                        if i < tokens.len() {
                            out.push(tokens[i].clone());
                        }
                    }
                }
                _ => {}
            }
            i += 1;
            continue;
        }
        if i + 1 < tokens.len()
            && tokens[i + 1].parse::<f64>().is_ok()
            && token.parse::<f64>().is_ok()
        {
            let x = token.parse::<f64>().unwrap_or(0.0);
            let y = tokens[i + 1].parse::<f64>().unwrap_or(0.0);
            out.push(f(fx(x, y)));
            out.push(f(fy(x, y)));
            i += 2;
        } else {
            out.push(token.clone());
            i += 1;
        }
    }
    out.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_has_geometry() {
        for id in crate::opts::SHAPE_PRESETS {
            let opts = if *id == "custGeom" {
                GeomOpts {
                    points: Some(vec![
                        Point {
                            x: 0.0,
                            y: 0.0,
                            move_to: true,
                            ..Point::default()
                        },
                        Point {
                            x: 2.0,
                            y: 0.0,
                            ..Point::default()
                        },
                        Point {
                            x: 2.0,
                            y: 1.0,
                            close: true,
                            ..Point::default()
                        },
                    ]),
                    ..GeomOpts::default()
                }
            } else {
                GeomOpts::default()
            };
            let path = path(id, 2.0, 1.0, &opts);
            let path = path.unwrap_or_else(|| panic!("no geometry for `{id}`"));
            assert!(!path.is_empty(), "empty geometry for `{id}`");
            assert!(
                path.starts_with('M') || path.starts_with('m'),
                "`{id}` does not start with a move: {path}"
            );
        }
    }

    #[test]
    fn custom_geometry_follows_points() {
        let opts = GeomOpts {
            points: Some(vec![
                Point {
                    x: 0.0,
                    y: 0.0,
                    move_to: true,
                    ..Point::default()
                },
                Point {
                    x: 2.0,
                    y: 0.0,
                    ..Point::default()
                },
                Point {
                    x: 2.0,
                    y: 1.0,
                    close: true,
                    ..Point::default()
                },
            ]),
            ..GeomOpts::default()
        };
        let path = path("custGeom", 2.0, 1.0, &opts).unwrap();
        assert!(path.contains("M 0 0"));
        assert!(path.contains("L 2 0"));
        assert!(path.trim_end().ends_with('Z'));
    }

    #[test]
    fn open_lines_have_no_fill() {
        assert!(is_open_line("line"));
        assert!(is_open_line("lineInv"));
        assert!(is_open_line("arc"));
        assert!(!is_open_line("rect"));
    }
}
