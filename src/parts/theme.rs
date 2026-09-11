//! `ppt/theme/theme1.xml`. The color scheme and format scheme are the standard
//! Office scaffold; only the font faces are configurable. Nothing generated
//! references scheme colors — slides carry explicit `srgbClr` values — so this
//! part exists to keep the package valid and PowerPoint-happy. It is written at
//! full Office size (script fonts, object defaults) because PowerPoint's
//! validators reject a trimmed-down theme.

use crate::config::Theme;
use crate::ooxml::Elem;

pub const URI: &str = "ppt/theme/theme1.xml";

/// The notes master gets its own theme part, as real decks do.
pub const NOTES_URI: &str = "ppt/theme/theme2.xml";

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

/// The standard Office script-font fallbacks, present in every full theme.
const SCRIPT_FONTS: [(&str, &str); 30] = [
    ("Jpan", "ＭＳ Ｐゴシック"),
    ("Hang", "맑은 고딕"),
    ("Hans", "宋体"),
    ("Hant", "新細明體"),
    ("Arab", "Times New Roman"),
    ("Hebr", "Times New Roman"),
    ("Thai", "Angsana New"),
    ("Ethi", "Nyala"),
    ("Beng", "Vrinda"),
    ("Gujr", "Shruti"),
    ("Khmr", "MoolBoran"),
    ("Knda", "Tunga"),
    ("Guru", "Raavi"),
    ("Cans", "Euphemia"),
    ("Cher", "Plantagenet Cherokee"),
    ("Yiii", "Microsoft Yi Baiti"),
    ("Tibt", "Microsoft Himalaya"),
    ("Thaa", "MV Boli"),
    ("Deva", "Mangal"),
    ("Telu", "Gautami"),
    ("Taml", "Latha"),
    ("Syrc", "Estrangelo Edessa"),
    ("Orya", "Kalinga"),
    ("Mlym", "Kartika"),
    ("Laoo", "DokChampa"),
    ("Sinh", "Iskoola Pota"),
    ("Mong", "Mongolian Baiti"),
    ("Viet", "Times New Roman"),
    ("Uigh", "Microsoft Uighur"),
    ("Geor", "Sylfaen"),
];

pub fn build(theme: &Theme) -> Vec<u8> {
    let root = Elem::new("a:theme")
        .attr(
            "xmlns:a",
            "http://schemas.openxmlformats.org/drawingml/2006/main",
        )
        .attr("name", "Office Theme")
        .child(
            Elem::new("a:themeElements")
                .child(clr_scheme())
                .child(font_scheme(theme))
                .child(fmt_scheme()),
        )
        .child(object_defaults())
        .child(Elem::new("a:extraClrSchemeLst"));
    root.to_xml()
}

fn clr_scheme() -> Elem {
    Elem::new("a:clrScheme")
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
        }))
}

fn font_scheme(theme: &Theme) -> Elem {
    Elem::new("a:fontScheme")
        .attr("name", "Office")
        .child(font("a:majorFont", &theme.major_font))
        .child(font("a:minorFont", &theme.minor_font))
}

fn font(name: &str, typeface: &str) -> Elem {
    let mut elem = Elem::new(name)
        .child(Elem::new("a:latin").attr("typeface", typeface))
        .child(Elem::new("a:ea").attr("typeface", ""))
        .child(Elem::new("a:cs").attr("typeface", ""));
    for (script, face) in SCRIPT_FONTS {
        elem = elem.child(
            Elem::new("a:font")
                .attr("script", script)
                .attr("typeface", face),
        );
    }
    elem
}

fn fmt_scheme() -> Elem {
    Elem::new("a:fmtScheme")
        .attr("name", "Office")
        .child(
            Elem::new("a:fillStyleLst")
                .child(solid_scheme("phClr"))
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
                    .child(solid_scheme("phClr"))
                    .child(Elem::new("a:prstDash").attr("val", "solid"))
            })),
        )
        .child(
            Elem::new("a:effectStyleLst")
                .child(Elem::new("a:effectStyle").child(Elem::new("a:effectLst")))
                .child(
                    Elem::new("a:effectStyle").child(
                        Elem::new("a:effectLst").child(outer_shadow("40000", "20000", "38000")),
                    ),
                )
                .child(
                    Elem::new("a:effectStyle").child(
                        Elem::new("a:effectLst")
                            .child(outer_shadow("40000", "23000", "35000"))
                            .child(inner_shadow("40000", "23000", "35000")),
                    ),
                ),
        )
        .child(
            Elem::new("a:bgFillStyleLst")
                .child(solid_scheme("phClr"))
                .child(grad_fill("102000", "0"))
                .child(grad_fill("100000", "0")),
        )
}

fn solid_scheme(val: &str) -> Elem {
    Elem::new("a:solidFill").child(Elem::new("a:schemeClr").attr("val", val))
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

fn outer_shadow(blur: &str, dist: &str, alpha: &str) -> Elem {
    Elem::new("a:outerShdw")
        .attr("blurRad", blur)
        .attr("dist", dist)
        .attr("dir", "5400000")
        .attr("rotWithShape", "0")
        .child(black_alpha(alpha))
}

fn inner_shadow(blur: &str, dist: &str, alpha: &str) -> Elem {
    Elem::new("a:innerShdw")
        .attr("blurRad", blur)
        .attr("dist", dist)
        .attr("dir", "5400000")
        .child(black_alpha(alpha))
}

fn black_alpha(alpha: &str) -> Elem {
    Elem::new("a:srgbClr")
        .attr("val", "000000")
        .child(Elem::new("a:alpha").attr("val", alpha))
}

fn object_defaults() -> Elem {
    Elem::new("a:objectDefaults")
        .child(
            Elem::new("a:spDef")
                .child(
                    Elem::new("a:spPr")
                        .child(solid_scheme("accent1"))
                        .child(Elem::new("a:ln").child(Elem::new("a:noFill"))),
                )
                .child(
                    Elem::new("a:bodyPr")
                        .attr("rot", "0")
                        .attr("spcFirstLastPara", "0")
                        .attr("vertOverflow", "overflow")
                        .attr("horzOverflow", "overflow")
                        .attr("vert", "horz")
                        .attr("wrap", "square")
                        .attr("lIns", "91440")
                        .attr("tIns", "45720")
                        .attr("rIns", "91440")
                        .attr("bIns", "45720")
                        .attr("numCol", "1")
                        .attr("spcCol", "0")
                        .attr("rtlCol", "0")
                        .attr("fromWordArt", "0")
                        .attr("anchor", "ctr")
                        .attr("anchorCtr", "0")
                        .attr("forceAA", "0")
                        .attr("compatLnSpc", "1")
                        .child(
                            Elem::new("a:prstTxWarp")
                                .attr("prst", "textNoShape")
                                .child(Elem::new("a:avLst")),
                        )
                        .child(Elem::new("a:noAutofit")),
                )
                .child(
                    Elem::new("a:lstStyle").child(
                        Elem::new("a:defPPr")
                            .attr("algn", "ctr")
                            .child(Elem::new("a:defRPr").attr("dirty", "0")),
                    ),
                )
                .child(
                    Elem::new("a:style")
                        .child(
                            Elem::new("a:lnRef").attr("idx", "2").child(
                                Elem::new("a:schemeClr")
                                    .attr("val", "accent1")
                                    .child(Elem::new("a:shade").attr("val", "50000")),
                            ),
                        )
                        .child(
                            Elem::new("a:fillRef")
                                .attr("idx", "1")
                                .child(Elem::new("a:schemeClr").attr("val", "accent1")),
                        )
                        .child(
                            Elem::new("a:effectRef")
                                .attr("idx", "0")
                                .child(Elem::new("a:schemeClr").attr("val", "accent1")),
                        )
                        .child(
                            Elem::new("a:fontRef")
                                .attr("idx", "minor")
                                .child(Elem::new("a:schemeClr").attr("val", "lt1")),
                        ),
                ),
        )
        .child(
            Elem::new("a:txDef")
                .child(Elem::new("a:spPr").child(Elem::new("a:noFill")))
                .child(
                    Elem::new("a:bodyPr")
                        .attr("wrap", "square")
                        .attr("rtlCol", "0")
                        .child(Elem::new("a:spAutoFit")),
                )
                .child(
                    Elem::new("a:lstStyle").child(
                        Elem::new("a:defPPr").attr("algn", "l").child(
                            Elem::new("a:defRPr")
                                .attr("lang", "en-US")
                                .attr("sz", "1800")
                                .attr("dirty", "0"),
                        ),
                    ),
                ),
        )
}
