//! `ppt/theme/theme1.xml`. The color scheme and format scheme are the standard
//! Office scaffold; only the font faces are configurable. Nothing generated
//! references scheme colors — slides carry explicit `srgbClr` values — so this
//! part exists to keep the package valid and PowerPoint-happy.

use crate::config::Theme;
use crate::ooxml::Elem;

pub const URI: &str = "ppt/theme/theme1.xml";

const COLORS: [(&str, &str); 12] = [
    ("dk1", "sys:windowText:000000"),
    ("lt1", "sys:window:FFFFFF"),
    ("dk2", "44546A"),
    ("lt2", "E7E6E6"),
    ("accent1", "4472C4"),
    ("accent2", "ED7D31"),
    ("accent3", "A5A5A5"),
    ("accent4", "FFC000"),
    ("accent5", "5B9BD5"),
    ("accent6", "70AD47"),
    ("hlink", "0563C1"),
    ("folHlink", "954F72"),
];

pub fn build(theme: &Theme) -> Vec<u8> {
    let clr_scheme = Elem::new("a:clrScheme")
        .attr("name", "Office")
        .children(COLORS.iter().map(|(name, value)| {
            let inner = match value.strip_prefix("sys:") {
                Some(rest) => {
                    let (val, last) = rest.split_once(':').unwrap_or((rest, ""));
                    Elem::new("a:sysClr").attr("val", val).attr("lastClr", last)
                }
                None => Elem::new("a:srgbClr").attr("val", *value),
            };
            Elem::new(format!("a:{name}")).child(inner)
        }));

    let font_scheme = Elem::new("a:fontScheme")
        .attr("name", "Office")
        .child(font("a:majorFont", &theme.major_font))
        .child(font("a:minorFont", &theme.minor_font));

    let root = Elem::new("a:theme")
        .attr(
            "xmlns:a",
            "http://schemas.openxmlformats.org/drawingml/2006/main",
        )
        .attr("name", "Office Theme")
        .child(
            Elem::new("a:themeElements")
                .child(clr_scheme)
                .child(font_scheme)
                .child(fmt_scheme()),
        );
    root.to_xml()
}

fn font(name: &str, typeface: &str) -> Elem {
    Elem::new(name)
        .child(Elem::new("a:latin").attr("typeface", typeface))
        .child(Elem::new("a:ea").attr("typeface", ""))
        .child(Elem::new("a:cs").attr("typeface", ""))
}

fn fmt_scheme() -> Elem {
    Elem::new("a:fmtScheme")
        .attr("name", "Office")
        .child(
            Elem::new("a:fillStyleLst")
                .child(
                    Elem::new("a:solidFill").child(Elem::new("a:schemeClr").attr("val", "phClr")),
                )
                .child(grad_fill("30000", "70000"))
                .child(grad_fill("100000", "0")),
        )
        .child(
            Elem::new("a:lnStyleLst").children(["6350", "12700", "19050"].into_iter().map(|w| {
                Elem::new("a:ln")
                    .attr("w", w)
                    .attr("cap", "flat")
                    .attr("cmpd", "sng")
                    .attr("algn", "ctr")
                    .child(
                        Elem::new("a:solidFill")
                            .child(Elem::new("a:schemeClr").attr("val", "phClr")),
                    )
                    .child(Elem::new("a:prstDash").attr("val", "solid"))
            })),
        )
        .child(
            Elem::new("a:effectStyleLst")
                .child(Elem::new("a:effectStyle").child(Elem::new("a:effectLst")))
                .child(Elem::new("a:effectStyle").child(effect_lst("50800")))
                .child(Elem::new("a:effectStyle").child(effect_lst("50800"))),
        )
        .child(
            Elem::new("a:bgFillStyleLst")
                .child(
                    Elem::new("a:solidFill").child(Elem::new("a:schemeClr").attr("val", "phClr")),
                )
                .child(grad_fill("102000", "0"))
                .child(grad_fill("100000", "0")),
        )
}

fn grad_fill(lum_mod: &str, lum_off: &str) -> Elem {
    Elem::new("a:gradFill")
        .attr("rotWithShape", "1")
        .child(
            Elem::new("a:gsLst")
                .child(
                    Elem::new("a:gs").attr("pos", "0").child(
                        Elem::new("a:schemeClr")
                            .attr("val", "phClr")
                            .child(Elem::new("a:lumMod").attr("val", lum_mod))
                            .child(Elem::new("a:lumOff").attr("val", lum_off)),
                    ),
                )
                .child(
                    Elem::new("a:gs")
                        .attr("pos", "100000")
                        .child(Elem::new("a:schemeClr").attr("val", "phClr")),
                ),
        )
        .child(
            Elem::new("a:lin")
                .attr("ang", "5400000")
                .attr("scaled", "0"),
        )
}

fn effect_lst(blur: &str) -> Elem {
    Elem::new("a:effectLst").child(
        Elem::new("a:outerShdw")
            .attr("blurRad", blur)
            .attr("dist", "38100")
            .attr("dir", "5400000")
            .attr("rotWithShape", "0")
            .child(
                Elem::new("a:srgbClr")
                    .attr("val", "000000")
                    .child(Elem::new("a:alpha").attr("val", "40000")),
            ),
    )
}
