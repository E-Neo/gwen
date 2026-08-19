use std::io::Write;

use quick_xml::Writer;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};

use crate::dto::*;

pub fn txbody_to_xml(tf: &TextFrameDto) -> String {
    let mut writer = Writer::new(Vec::new());

    write_txbody(tf, &mut writer);

    String::from_utf8(writer.into_inner()).expect("valid UTF-8")
}

fn write_txbody(tf: &TextFrameDto, writer: &mut Writer<Vec<u8>>) {
    write_body_pr(tf, writer);

    match &tf.default_paragraph_style {
        Some(dps) => write_default_paragraph_style(dps, writer),
        None => {
            writer
                .write_event(Event::Empty(BytesStart::new("a:lstStyle")))
                .ok();
        }
    }

    for p in &tf.paragraphs {
        write_paragraph(p, writer);
    }
}

fn write_body_pr(tf: &TextFrameDto, writer: &mut Writer<Vec<u8>>) {
    let has_any = tf.word_wrap.is_some()
        || tf.vertical_anchor.is_some()
        || tf.margin_left.is_some()
        || tf.margin_right.is_some()
        || tf.margin_top.is_some()
        || tf.margin_bottom.is_some()
        || tf.auto_size.is_some();

    let mut body_pr = BytesStart::new("a:bodyPr");

    if let Some(wrap) = tf.word_wrap {
        body_pr.push_attribute(("wrap", if wrap { "square" } else { "none" }));
    }
    if let Some(ref anchor) = tf.vertical_anchor {
        let val = match anchor {
            VerticalAnchor::Top => "t",
            VerticalAnchor::Middle => "ctr",
            VerticalAnchor::Bottom => "b",
            VerticalAnchor::Justified => "just",
            VerticalAnchor::Distributed => "dist",
        };
        body_pr.push_attribute(("anchor", val));
    }
    if let Some(v) = tf.margin_left {
        body_pr.push_attribute(("lIns", v.to_string().as_str()));
    }
    if let Some(v) = tf.margin_right {
        body_pr.push_attribute(("rIns", v.to_string().as_str()));
    }
    if let Some(v) = tf.margin_top {
        body_pr.push_attribute(("tIns", v.to_string().as_str()));
    }
    if let Some(v) = tf.margin_bottom {
        body_pr.push_attribute(("bIns", v.to_string().as_str()));
    }

    if !has_any {
        writer.write_event(Event::Empty(body_pr)).ok();
        return;
    }

    writer.write_event(Event::Start(body_pr)).ok();

    match tf.auto_size {
        Some(MsoAutoSize::TextToFitShape) => {
            writer
                .write_event(Event::Empty(BytesStart::new("a:spAutoFit")))
                .ok();
        }
        Some(MsoAutoSize::ShapeToFitText) => {
            writer
                .write_event(Event::Empty(BytesStart::new("a:normAutofit")))
                .ok();
        }
        Some(MsoAutoSize::None) => {
            writer
                .write_event(Event::Empty(BytesStart::new("a:noAutofit")))
                .ok();
        }
        None => {}
    }

    writer
        .write_event(Event::End(BytesEnd::new("a:bodyPr")))
        .ok();
}

pub(crate) fn write_paragraph(p: &ParagraphDto, writer: &mut Writer<Vec<u8>>) {
    writer
        .write_event(Event::Start(BytesStart::new("a:p")))
        .ok();

    let has_props = p.alignment.is_some()
        || p.line_spacing.is_some()
        || p.space_before.is_some()
        || p.space_after.is_some()
        || p.level.is_some_and(|l| l != 0);

    if has_props {
        let mut ppr = BytesStart::new("a:pPr");

        if let Some(ref algn) = p.alignment {
            let val = match algn {
                Alignment::Left => "l",
                Alignment::Center => "ctr",
                Alignment::Right => "r",
                Alignment::Justify => "just",
                Alignment::Distribute => "dist",
                Alignment::ThaiDistribute => "thaiDist",
                Alignment::JustifiedLow => "justLow",
            };
            ppr.push_attribute(("algn", val));
        }

        writer.write_event(Event::Start(ppr)).ok();

        write_p_pr_children(p, writer);

        writer.write_event(Event::End(BytesEnd::new("a:pPr"))).ok();
    }

    for r in &p.runs {
        write_run(r, writer);
    }

    let end_font = p
        .font
        .as_ref()
        .or_else(|| p.runs.last().and_then(|r| r.font.as_ref()));
    if let Some(font) = end_font {
        writer
            .write_event(Event::Start(BytesStart::new("a:endParaRPr")))
            .ok();
        write_font_children(font, writer);
        writer
            .write_event(Event::End(BytesEnd::new("a:endParaRPr")))
            .ok();
    }

    writer.write_event(Event::End(BytesEnd::new("a:p"))).ok();
}

fn write_p_pr_children(p: &ParagraphDto, writer: &mut Writer<Vec<u8>>) {
    if let Some(ls) = p.line_spacing {
        writer
            .write_event(Event::Start(BytesStart::new("a:lnSpc")))
            .ok();
        if (0.5..=10.0).contains(&ls) {
            let mut pct = BytesStart::new("a:spcPct");
            let val = (ls * 100000.0).round() as i64;
            pct.push_attribute(("val", val.to_string().as_str()));
            writer.write_event(Event::Empty(pct)).ok();
        } else {
            let mut pts = BytesStart::new("a:spcPts");
            let val = (ls * 100.0).round() as i64;
            pts.push_attribute(("val", val.to_string().as_str()));
            writer.write_event(Event::Empty(pts)).ok();
        }
        writer
            .write_event(Event::End(BytesEnd::new("a:lnSpc")))
            .ok();
    }

    if let Some(sb) = p.space_before {
        writer
            .write_event(Event::Start(BytesStart::new("a:spcBef")))
            .ok();
        let mut pts = BytesStart::new("a:spcPts");
        pts.push_attribute(("val", sb.to_string().as_str()));
        writer.write_event(Event::Empty(pts)).ok();
        writer
            .write_event(Event::End(BytesEnd::new("a:spcBef")))
            .ok();
    }

    if let Some(sa) = p.space_after {
        writer
            .write_event(Event::Start(BytesStart::new("a:spcAft")))
            .ok();
        let mut pts = BytesStart::new("a:spcPts");
        pts.push_attribute(("val", sa.to_string().as_str()));
        writer.write_event(Event::Empty(pts)).ok();
        writer
            .write_event(Event::End(BytesEnd::new("a:spcAft")))
            .ok();
    }
}

fn write_default_paragraph_style(dps: &ParagraphDto, writer: &mut Writer<Vec<u8>>) {
    writer
        .write_event(Event::Start(BytesStart::new("a:lstStyle")))
        .ok();

    let mut ppr = BytesStart::new("a:lvl1pPr");

    if let Some(ref algn) = dps.alignment {
        let val = match algn {
            Alignment::Left => "l",
            Alignment::Center => "ctr",
            Alignment::Right => "r",
            Alignment::Justify => "just",
            Alignment::Distribute => "dist",
            Alignment::ThaiDistribute => "thaiDist",
            Alignment::JustifiedLow => "justLow",
        };
        ppr.push_attribute(("algn", val));
    }

    writer.write_event(Event::Start(ppr)).ok();

    write_p_pr_children(dps, writer);

    if let Some(ref font) = dps.font {
        let mut def = BytesStart::new("a:defRPr");
        if let Some(sz) = font.size {
            def.push_attribute(("sz", sz.to_string().as_str()));
        }
        if let Some(b) = font.bold {
            def.push_attribute(("b", if b { "1" } else { "0" }));
        }
        if let Some(i) = font.italic {
            def.push_attribute(("i", if i { "1" } else { "0" }));
        }
        if let Some(u) = font.underline {
            def.push_attribute(("u", if u { "sng" } else { "none" }));
        }
        writer.write_event(Event::Start(def)).ok();
        write_font_children(font, writer);
        writer
            .write_event(Event::End(BytesEnd::new("a:defRPr")))
            .ok();
    }

    writer
        .write_event(Event::End(BytesEnd::new("a:lvl1pPr")))
        .ok();
    writer
        .write_event(Event::End(BytesEnd::new("a:lstStyle")))
        .ok();
}

pub(crate) fn write_run(r: &RunDto, writer: &mut Writer<Vec<u8>>) {
    writer
        .write_event(Event::Start(BytesStart::new("a:r")))
        .ok();

    if let Some(ref font) = r.font {
        write_rpr(font, writer);
    }

    if let Some(ref hlink) = r.hyperlink {
        let mut elem = BytesStart::new("a:hlinkClick");
        if let Some(ref rid) = hlink.r_id {
            elem.push_attribute(("r:id", rid.as_str()));
        }
        if let Some(ref tip) = hlink.tooltip {
            elem.push_attribute(("tooltip", tip.as_str()));
        }
        if let Some(ref addr) = hlink.address {
            elem.push_attribute(("address", addr.as_str()));
        }
        writer.write_event(Event::Empty(elem)).ok();
    }

    writer
        .write_event(Event::Start(BytesStart::new("a:t")))
        .ok();
    writer
        .write_event(Event::Text(BytesText::new(&r.text)))
        .ok();
    writer.write_event(Event::End(BytesEnd::new("a:t"))).ok();

    writer.write_event(Event::End(BytesEnd::new("a:r"))).ok();
}

pub(crate) fn write_rpr(font: &FontDto, writer: &mut Writer<Vec<u8>>) {
    let has_rpr = font.name.is_some()
        || font.size.is_some()
        || font.bold.is_some()
        || font.italic.is_some()
        || font.underline.is_some()
        || font.color.is_some();

    if !has_rpr {
        return;
    }

    let mut rpr = BytesStart::new("a:rPr");

    if let Some(sz) = font.size {
        rpr.push_attribute(("sz", sz.to_string().as_str()));
    }
    if let Some(b) = font.bold {
        rpr.push_attribute(("b", if b { "1" } else { "0" }));
    }
    if let Some(i) = font.italic {
        rpr.push_attribute(("i", if i { "1" } else { "0" }));
    }
    if let Some(u) = font.underline {
        rpr.push_attribute(("u", if u { "sng" } else { "none" }));
    }

    writer.write_event(Event::Start(rpr)).ok();
    write_font_children(font, writer);
    writer.write_event(Event::End(BytesEnd::new("a:rPr"))).ok();
}

fn write_font_children(font: &FontDto, writer: &mut Writer<Vec<u8>>) {
    if let Some(ref color) = font.color {
        write_color(color, writer);
    }
    if let Some(ref name) = font.name {
        let mut latin = BytesStart::new("a:latin");
        latin.push_attribute(("typeface", name.as_str()));
        writer.write_event(Event::Empty(latin)).ok();
        let mut ea = BytesStart::new("a:ea");
        ea.push_attribute(("typeface", name.as_str()));
        writer.write_event(Event::Empty(ea)).ok();
    }
}

fn write_color(color: &ColorFormatDto, writer: &mut Writer<Vec<u8>>) {
    let has_fill = color.color_type.is_some() || color.rgb.is_some() || color.theme_color.is_some();

    if !has_fill {
        return;
    }

    writer
        .write_event(Event::Start(BytesStart::new("a:solidFill")))
        .ok();

    match color.color_type {
        Some(ColorType::Scheme) | None if color.theme_color.is_some() => {
            let mut clr = BytesStart::new("a:schemeClr");
            if let Some(ref tc) = color.theme_color {
                clr.push_attribute(("val", tc.as_str()));
            }
            writer.write_event(Event::Empty(clr)).ok();
        }
        Some(ColorType::Rgb) | None if color.rgb.is_some() => {
            let mut clr = BytesStart::new("a:srgbClr");
            if let Some(ref rgb) = color.rgb {
                clr.push_attribute(("val", rgb.as_str()));
            }
            writer.write_event(Event::Empty(clr)).ok();
        }
        _ => {
            let mut clr = BytesStart::new("a:schemeClr");
            clr.push_attribute(("val", "tx1"));
            writer.write_event(Event::Empty(clr)).ok();
        }
    }

    writer
        .write_event(Event::End(BytesEnd::new("a:solidFill")))
        .ok();
}

pub fn table_to_xml(table: &TableDto) -> String {
    let mut writer = Writer::new(Vec::new());
    writer
        .write_event(Event::Start(BytesStart::new("a:tbl")))
        .ok();
    writer
        .write_event(Event::Start(BytesStart::new("a:tblPr")))
        .ok();
    writer
        .write_event(Event::Empty(BytesStart::new("a:noFill")))
        .ok();
    writer
        .write_event(Event::End(BytesEnd::new("a:tblPr")))
        .ok();
    writer
        .write_event(Event::Start(BytesStart::new("a:tblGrid")))
        .ok();
    for col in &table.grid {
        let mut gc = BytesStart::new("a:gridCol");
        gc.push_attribute(("w", col.width.to_string().as_str()));
        writer.write_event(Event::Empty(gc)).ok();
    }
    writer
        .write_event(Event::End(BytesEnd::new("a:tblGrid")))
        .ok();
    for row in &table.rows {
        let mut tr = BytesStart::new("a:tr");
        if let Some(h) = row.height {
            tr.push_attribute(("h", h.to_string().as_str()));
        }
        writer.write_event(Event::Start(tr)).ok();
        for cell in &row.cells {
            let mut tc = BytesStart::new("a:tc");
            if let Some(rs) = cell.row_span {
                tc.push_attribute(("rowSpan", rs.to_string().as_str()));
            }
            if let Some(gs) = cell.grid_span {
                tc.push_attribute(("gridSpan", gs.to_string().as_str()));
            }
            if let Some(hm) = cell.h_merge {
                tc.push_attribute(("hMerge", if hm { "1" } else { "0" }));
            }
            if let Some(vm) = cell.v_merge {
                tc.push_attribute(("vMerge", if vm { "1" } else { "0" }));
            }
            writer.write_event(Event::Start(tc)).ok();
            let body = if let Some(ref tf) = cell.text_frame {
                let mut w2 = Writer::new(Vec::new());
                w2.write_event(Event::Start(BytesStart::new("a:txBody")))
                    .ok();
                write_txbody(tf, &mut w2);
                w2.write_event(Event::End(BytesEnd::new("a:txBody"))).ok();
                w2.into_inner()
            } else {
                let mut w2 = Writer::new(Vec::new());
                w2.write_event(Event::Start(BytesStart::new("a:txBody")))
                    .ok();
                w2.write_event(Event::Empty(BytesStart::new("a:bodyPr")))
                    .ok();
                w2.write_event(Event::Start(BytesStart::new("a:p"))).ok();
                w2.write_event(Event::End(BytesEnd::new("a:p"))).ok();
                w2.write_event(Event::End(BytesEnd::new("a:txBody"))).ok();
                w2.into_inner()
            };
            writer.get_mut().write_all(&body).ok();
            writer.write_event(Event::End(BytesEnd::new("a:tc"))).ok();
        }
        writer.write_event(Event::End(BytesEnd::new("a:tr"))).ok();
    }
    writer.write_event(Event::End(BytesEnd::new("a:tbl"))).ok();
    String::from_utf8(writer.into_inner()).expect("valid UTF-8")
}

pub fn shape_to_xml(shape: &ShapeDto) -> String {
    let mut writer = Writer::new(Vec::new());
    match shape.shape_type {
        ShapeType::TextBox
        | ShapeType::AutoShape
        | ShapeType::Line
        | ShapeType::Freeform
        | ShapeType::Placeholder => write_sp_elem(shape, &mut writer),
        ShapeType::Picture => write_pic_elem(shape, &mut writer),
        ShapeType::Group => write_grp_sp_elem(shape, &mut writer),
        ShapeType::Table | ShapeType::Chart => write_graphic_frame_elem(shape, &mut writer),
        _ => write_sp_elem(shape, &mut writer),
    }
    String::from_utf8(writer.into_inner()).expect("valid UTF-8")
}

fn write_xfrm(shape: &ShapeDto, writer: &mut Writer<Vec<u8>>, tag: &str) {
    let rot_attr = shape.rotation.map(|r| (r * 60000.0).round() as i64);
    let l = shape.left.unwrap_or(0);
    let t = shape.top.unwrap_or(0);
    let w = shape.width.unwrap_or(914400);
    let h = shape.height.unwrap_or(274320);
    let mut elem = BytesStart::new(tag);
    if let Some(rv) = rot_attr {
        elem.push_attribute(("rot", rv.to_string().as_str()));
    }
    writer.write_event(Event::Start(elem)).ok();
    let mut off = BytesStart::new("a:off");
    off.push_attribute(("x", l.to_string().as_str()));
    off.push_attribute(("y", t.to_string().as_str()));
    writer.write_event(Event::Empty(off)).ok();
    let mut ext = BytesStart::new("a:ext");
    ext.push_attribute(("cx", w.to_string().as_str()));
    ext.push_attribute(("cy", h.to_string().as_str()));
    writer.write_event(Event::Empty(ext)).ok();
    writer.write_event(Event::End(BytesEnd::new(tag))).ok();
}

fn write_shape_fill_outline(shape: &ShapeDto, writer: &mut Writer<Vec<u8>>) {
    if let Some(ref fill) = shape.fill {
        match fill.fill_type {
            Some(FillType::NoFill) => {
                writer
                    .write_event(Event::Empty(BytesStart::new("a:noFill")))
                    .ok();
            }
            _ => {
                writer
                    .write_event(Event::Start(BytesStart::new("a:solidFill")))
                    .ok();
                if let Some(ref color) = fill.color {
                    if let Some(ref rgb) = color.rgb {
                        let mut clr = BytesStart::new("a:srgbClr");
                        clr.push_attribute(("val", rgb.as_str()));
                        writer.write_event(Event::Empty(clr)).ok();
                    } else if let Some(ref tc) = color.theme_color {
                        let mut clr = BytesStart::new("a:schemeClr");
                        clr.push_attribute(("val", tc.as_str()));
                        writer.write_event(Event::Empty(clr)).ok();
                    }
                }
                writer
                    .write_event(Event::End(BytesEnd::new("a:solidFill")))
                    .ok();
            }
        }
    }
    if let Some(ref outline) = shape.outline {
        let w_str = outline.width.map(|w| w.to_string());
        let has_any = outline.width.is_some()
            || outline.cap.is_some()
            || outline.compound.is_some()
            || outline.dash.is_some()
            || outline.fill.is_some();
        if has_any {
            let mut ln = BytesStart::new("a:ln");
            if let Some(ref ws) = w_str {
                ln.push_attribute(("w", ws.as_str()));
            }
            if let Some(ref cap) = outline.cap {
                ln.push_attribute((
                    "cap",
                    match cap {
                        LineCap::Rnd => "rnd",
                        LineCap::Sq => "sq",
                        LineCap::Flat => "flat",
                    },
                ));
            }
            if let Some(ref cmp) = outline.compound {
                ln.push_attribute((
                    "cmpd",
                    match cmp {
                        CompoundLine::Sng => "sng",
                        CompoundLine::Dbl => "dbl",
                        CompoundLine::ThickThin => "thickThin",
                        CompoundLine::ThinThick => "thinThick",
                        CompoundLine::Tri => "tri",
                    },
                ));
            }
            writer.write_event(Event::Start(ln)).ok();
            if let Some(ref dash) = outline.dash {
                let mut pd = BytesStart::new("a:prstDash");
                pd.push_attribute((
                    "val",
                    match dash {
                        LineDash::Solid => "solid",
                        LineDash::Dot => "dot",
                        LineDash::Dash => "dash",
                        LineDash::LgDash => "lgDash",
                        LineDash::DashDot => "dashDot",
                        LineDash::LgDashDot => "lgDashDot",
                        LineDash::LgDashDotDot => "lgDashDotDot",
                        LineDash::SysDash => "sysDash",
                        LineDash::SysDot => "sysDot",
                        LineDash::SysDashDot => "sysDashDot",
                        LineDash::SysDashDotDot => "sysDashDotDot",
                    },
                ));
                writer.write_event(Event::Empty(pd)).ok();
            }
            if let Some(ref fill) = outline.fill {
                match fill.fill_type {
                    Some(FillType::NoFill) => {
                        writer
                            .write_event(Event::Empty(BytesStart::new("a:noFill")))
                            .ok();
                    }
                    _ => {
                        writer
                            .write_event(Event::Start(BytesStart::new("a:solidFill")))
                            .ok();
                        if let Some(ref color) = fill.color {
                            if let Some(ref rgb) = color.rgb {
                                let mut clr = BytesStart::new("a:srgbClr");
                                clr.push_attribute(("val", rgb.as_str()));
                                writer.write_event(Event::Empty(clr)).ok();
                            } else if let Some(ref tc) = color.theme_color {
                                let mut clr = BytesStart::new("a:schemeClr");
                                clr.push_attribute(("val", tc.as_str()));
                                writer.write_event(Event::Empty(clr)).ok();
                            }
                        }
                        writer
                            .write_event(Event::End(BytesEnd::new("a:solidFill")))
                            .ok();
                    }
                }
            }
            writer.write_event(Event::End(BytesEnd::new("a:ln"))).ok();
        }
    }
}

fn write_sp_elem(shape: &ShapeDto, writer: &mut Writer<Vec<u8>>) {
    writer
        .write_event(Event::Start(BytesStart::new("p:sp")))
        .ok();
    let name = shape.name.as_deref().unwrap_or("");
    writer
        .write_event(Event::Start(BytesStart::new("p:nvSpPr")))
        .ok();
    let mut cid = BytesStart::new("p:cNvPr");
    cid.push_attribute(("id", shape.shape_id.to_string().as_str()));
    cid.push_attribute(("name", name));
    writer.write_event(Event::Empty(cid)).ok();
    let mut sp = BytesStart::new("p:cNvSpPr");
    if matches!(shape.shape_type, ShapeType::TextBox) {
        sp.push_attribute(("txBox", "1"));
    }
    writer.write_event(Event::Empty(sp)).ok();
    if shape.is_placeholder {
        writer
            .write_event(Event::Start(BytesStart::new("p:nvPr")))
            .ok();
        if let Some(ref ph) = shape.placeholder_format {
            let mut pe = BytesStart::new("p:ph");
            pe.push_attribute(("idx", ph.idx.to_string().as_str()));
            if let Some(ref pt) = ph.ph_type {
                let s = match pt {
                    PlaceholderType::Title => "title",
                    PlaceholderType::Body => "body",
                    PlaceholderType::CenterTitle => "ctrTitle",
                    PlaceholderType::SubTitle => "subTitle",
                    PlaceholderType::Object => "obj",
                    _ => "obj",
                };
                pe.push_attribute(("type", s));
            }
            writer.write_event(Event::Empty(pe)).ok();
        }
        writer.write_event(Event::End(BytesEnd::new("p:nvPr"))).ok();
    } else {
        writer
            .write_event(Event::Empty(BytesStart::new("p:nvPr")))
            .ok();
    }
    writer
        .write_event(Event::End(BytesEnd::new("p:nvSpPr")))
        .ok();

    writer
        .write_event(Event::Start(BytesStart::new("p:spPr")))
        .ok();
    write_xfrm(shape, writer, "a:xfrm");
    let prst = shape.auto_shape_type.as_deref().unwrap_or("rect");
    let mut pg = BytesStart::new("a:prstGeom");
    pg.push_attribute(("prst", prst));
    writer.write_event(Event::Empty(pg)).ok();
    write_shape_fill_outline(shape, writer);
    writer.write_event(Event::End(BytesEnd::new("p:spPr"))).ok();

    if let Some(ref tf) = shape.text_frame {
        writer.get_mut().write_all(b"<p:txBody>").ok();
        writer
            .get_mut()
            .write_all(txbody_to_xml(tf).as_bytes())
            .ok();
        writer.get_mut().write_all(b"</p:txBody>").ok();
    }

    writer.write_event(Event::End(BytesEnd::new("p:sp"))).ok();
}

fn write_pic_elem(shape: &ShapeDto, writer: &mut Writer<Vec<u8>>) {
    writer
        .write_event(Event::Start(BytesStart::new("p:pic")))
        .ok();
    let name = shape.name.as_deref().unwrap_or("");
    writer
        .write_event(Event::Start(BytesStart::new("p:nvPicPr")))
        .ok();
    let mut cid = BytesStart::new("p:cNvPr");
    cid.push_attribute(("id", shape.shape_id.to_string().as_str()));
    cid.push_attribute(("name", name));
    writer.write_event(Event::Empty(cid)).ok();
    writer
        .write_event(Event::Empty(BytesStart::new("p:cNvPicPr")))
        .ok();
    writer
        .write_event(Event::Empty(BytesStart::new("p:nvPr")))
        .ok();
    writer
        .write_event(Event::End(BytesEnd::new("p:nvPicPr")))
        .ok();

    writer
        .write_event(Event::Start(BytesStart::new("p:blipFill")))
        .ok();
    let img = shape.image.as_deref().unwrap_or("rId1");
    let mut blip = BytesStart::new("a:blip");
    blip.push_attribute(("r:embed", img));
    writer.write_event(Event::Empty(blip)).ok();
    writer
        .write_event(Event::Start(BytesStart::new("a:stretch")))
        .ok();
    writer
        .write_event(Event::Empty(BytesStart::new("a:fillRect")))
        .ok();
    writer
        .write_event(Event::End(BytesEnd::new("a:stretch")))
        .ok();
    writer
        .write_event(Event::End(BytesEnd::new("p:blipFill")))
        .ok();

    writer
        .write_event(Event::Start(BytesStart::new("p:spPr")))
        .ok();
    write_xfrm(shape, writer, "a:xfrm");
    let mut pg = BytesStart::new("a:prstGeom");
    pg.push_attribute(("prst", "rect"));
    writer.write_event(Event::Empty(pg)).ok();
    write_shape_fill_outline(shape, writer);
    writer.write_event(Event::End(BytesEnd::new("p:spPr"))).ok();

    writer.write_event(Event::End(BytesEnd::new("p:pic"))).ok();
}

fn write_graphic_frame_elem(shape: &ShapeDto, writer: &mut Writer<Vec<u8>>) {
    writer
        .write_event(Event::Start(BytesStart::new("p:graphicFrame")))
        .ok();
    let name = shape.name.as_deref().unwrap_or("");
    writer
        .write_event(Event::Start(BytesStart::new("p:nvGraphicFramePr")))
        .ok();
    let mut cid = BytesStart::new("p:cNvPr");
    cid.push_attribute(("id", shape.shape_id.to_string().as_str()));
    cid.push_attribute(("name", name));
    writer.write_event(Event::Empty(cid)).ok();
    writer
        .write_event(Event::Empty(BytesStart::new("p:cNvGraphicFramePr")))
        .ok();
    writer
        .write_event(Event::Empty(BytesStart::new("p:nvPr")))
        .ok();
    writer
        .write_event(Event::End(BytesEnd::new("p:nvGraphicFramePr")))
        .ok();

    write_xfrm(shape, writer, "p:xfrm");

    writer
        .write_event(Event::Start(BytesStart::new("a:graphic")))
        .ok();
    let uri = if matches!(shape.shape_type, ShapeType::Table) {
        "http://schemas.openxmlformats.org/drawingml/2006/table"
    } else {
        "http://schemas.openxmlformats.org/drawingml/2006/chart"
    };
    let mut gd = BytesStart::new("a:graphicData");
    gd.push_attribute(("uri", uri));
    writer.write_event(Event::Start(gd)).ok();

    if matches!(shape.shape_type, ShapeType::Table) {
        if let Some(ref tbl) = shape.table {
            writer
                .get_mut()
                .write_all(table_to_xml(tbl).as_bytes())
                .ok();
        }
    } else if let Some(ref ch) = shape.chart {
        let rid = ch.r_id.as_deref().unwrap_or("rId1");
        let mut ce = BytesStart::new("c:chart");
        ce.push_attribute(("r:id", rid));
        writer.write_event(Event::Empty(ce)).ok();
    }

    writer
        .write_event(Event::End(BytesEnd::new("a:graphicData")))
        .ok();
    writer
        .write_event(Event::End(BytesEnd::new("a:graphic")))
        .ok();
    writer
        .write_event(Event::End(BytesEnd::new("p:graphicFrame")))
        .ok();
}

fn write_grp_sp_elem(shape: &ShapeDto, writer: &mut Writer<Vec<u8>>) {
    writer
        .write_event(Event::Start(BytesStart::new("p:grpSp")))
        .ok();
    let name = shape.name.as_deref().unwrap_or("");
    writer
        .write_event(Event::Start(BytesStart::new("p:nvGrpSpPr")))
        .ok();
    let mut cid = BytesStart::new("p:cNvPr");
    cid.push_attribute(("id", shape.shape_id.to_string().as_str()));
    cid.push_attribute(("name", name));
    writer.write_event(Event::Empty(cid)).ok();
    writer
        .write_event(Event::Empty(BytesStart::new("p:cNvGrpSpPr")))
        .ok();
    writer
        .write_event(Event::Empty(BytesStart::new("p:nvPr")))
        .ok();
    writer
        .write_event(Event::End(BytesEnd::new("p:nvGrpSpPr")))
        .ok();

    writer
        .write_event(Event::Start(BytesStart::new("p:grpSpPr")))
        .ok();
    writer
        .write_event(Event::Start(BytesStart::new("a:xfrm")))
        .ok();
    let (off_x, off_y) = (
        shape.left.unwrap_or(0).to_string(),
        shape.top.unwrap_or(0).to_string(),
    );
    writer
        .write_event(Event::Empty(
            BytesStart::new("a:off")
                .with_attributes(vec![("x", off_x.as_str()), ("y", off_y.as_str())]),
        ))
        .ok();
    let (ext_cx, ext_cy) = (
        shape.width.unwrap_or(0).to_string(),
        shape.height.unwrap_or(0).to_string(),
    );
    writer
        .write_event(Event::Empty(BytesStart::new("a:ext").with_attributes(
            vec![("cx", ext_cx.as_str()), ("cy", ext_cy.as_str())],
        )))
        .ok();
    let (ch_off_x, ch_off_y) = (
        shape.ch_off_x.unwrap_or(0).to_string(),
        shape.ch_off_y.unwrap_or(0).to_string(),
    );
    writer
        .write_event(Event::Empty(BytesStart::new("a:chOff").with_attributes(
            vec![("x", ch_off_x.as_str()), ("y", ch_off_y.as_str())],
        )))
        .ok();
    let (ch_ext_cx, ch_ext_cy) = (
        shape
            .ch_ext_cx
            .unwrap_or_else(|| shape.width.unwrap_or(0))
            .to_string(),
        shape
            .ch_ext_cy
            .unwrap_or_else(|| shape.height.unwrap_or(0))
            .to_string(),
    );
    writer
        .write_event(Event::Empty(BytesStart::new("a:chExt").with_attributes(
            vec![("cx", ch_ext_cx.as_str()), ("cy", ch_ext_cy.as_str())],
        )))
        .ok();
    writer.write_event(Event::End(BytesEnd::new("a:xfrm"))).ok();
    writer
        .write_event(Event::End(BytesEnd::new("p:grpSpPr")))
        .ok();

    if let Some(ref children) = shape.shapes {
        for child in children {
            writer
                .get_mut()
                .write_all(shape_to_xml(child).as_bytes())
                .ok();
        }
    }

    writer
        .write_event(Event::End(BytesEnd::new("p:grpSp")))
        .ok();
}

/// Serialize a single chart series (`c:ser`).
pub(crate) fn chart_series_to_xml(series: &ChartSeriesDto, idx: usize) -> String {
    let mut writer = Writer::new(Vec::new());
    writer
        .write_event(Event::Start(BytesStart::new("c:ser")))
        .ok();
    let mut idx_el = BytesStart::new("c:idx");
    idx_el.push_attribute(("val", idx.to_string().as_str()));
    writer.write_event(Event::Empty(idx_el)).ok();
    let mut order_el = BytesStart::new("c:order");
    order_el.push_attribute(("val", idx.to_string().as_str()));
    writer.write_event(Event::Empty(order_el)).ok();

    if let Some(name) = &series.name {
        writer
            .write_event(Event::Start(BytesStart::new("c:tx")))
            .ok();
        writer
            .write_event(Event::Start(BytesStart::new("c:strRef")))
            .ok();
        writer
            .write_event(Event::Start(BytesStart::new("c:strCache")))
            .ok();
        let mut pt_count = BytesStart::new("c:ptCount");
        pt_count.push_attribute(("val", "1"));
        writer.write_event(Event::Empty(pt_count)).ok();
        writer
            .write_event(Event::Start(BytesStart::new("c:pt")))
            .ok();
        writer
            .write_event(Event::Start(BytesStart::new("c:v")))
            .ok();
        writer.write_event(Event::Text(BytesText::new(name))).ok();
        writer.write_event(Event::End(BytesEnd::new("c:v"))).ok();
        writer.write_event(Event::End(BytesEnd::new("c:pt"))).ok();
        writer
            .write_event(Event::End(BytesEnd::new("c:strCache")))
            .ok();
        writer
            .write_event(Event::End(BytesEnd::new("c:strRef")))
            .ok();
        writer.write_event(Event::End(BytesEnd::new("c:tx"))).ok();
    }

    writer
        .write_event(Event::Start(BytesStart::new("c:cat")))
        .ok();
    writer
        .write_event(Event::Start(BytesStart::new("c:strRef")))
        .ok();
    writer
        .write_event(Event::Start(BytesStart::new("c:strCache")))
        .ok();
    let mut pt_count = BytesStart::new("c:ptCount");
    pt_count.push_attribute(("val", series.categories.len().to_string().as_str()));
    writer.write_event(Event::Empty(pt_count)).ok();
    for (j, cat) in series.categories.iter().enumerate() {
        let mut pt = BytesStart::new("c:pt");
        pt.push_attribute(("idx", j.to_string().as_str()));
        writer.write_event(Event::Start(pt)).ok();
        writer
            .write_event(Event::Start(BytesStart::new("c:v")))
            .ok();
        writer.write_event(Event::Text(BytesText::new(cat))).ok();
        writer.write_event(Event::End(BytesEnd::new("c:v"))).ok();
        writer.write_event(Event::End(BytesEnd::new("c:pt"))).ok();
    }
    writer
        .write_event(Event::End(BytesEnd::new("c:strCache")))
        .ok();
    writer
        .write_event(Event::End(BytesEnd::new("c:strRef")))
        .ok();
    writer.write_event(Event::End(BytesEnd::new("c:cat"))).ok();

    writer
        .write_event(Event::Start(BytesStart::new("c:val")))
        .ok();
    writer
        .write_event(Event::Start(BytesStart::new("c:numRef")))
        .ok();
    writer
        .write_event(Event::Start(BytesStart::new("c:numCache")))
        .ok();
    writer
        .write_event(Event::Start(BytesStart::new("c:formatCode")))
        .ok();
    writer
        .write_event(Event::Text(BytesText::new("General")))
        .ok();
    writer
        .write_event(Event::End(BytesEnd::new("c:formatCode")))
        .ok();
    let mut pt_count = BytesStart::new("c:ptCount");
    pt_count.push_attribute(("val", series.values.len().to_string().as_str()));
    writer.write_event(Event::Empty(pt_count)).ok();
    for (j, val) in series.values.iter().enumerate() {
        let mut pt = BytesStart::new("c:pt");
        pt.push_attribute(("idx", j.to_string().as_str()));
        writer.write_event(Event::Start(pt)).ok();
        writer
            .write_event(Event::Start(BytesStart::new("c:v")))
            .ok();
        writer
            .write_event(Event::Text(BytesText::new(&val.to_string())))
            .ok();
        writer.write_event(Event::End(BytesEnd::new("c:v"))).ok();
        writer.write_event(Event::End(BytesEnd::new("c:pt"))).ok();
    }
    writer
        .write_event(Event::End(BytesEnd::new("c:numCache")))
        .ok();
    writer
        .write_event(Event::End(BytesEnd::new("c:numRef")))
        .ok();
    writer.write_event(Event::End(BytesEnd::new("c:val"))).ok();

    writer.write_event(Event::End(BytesEnd::new("c:ser"))).ok();
    String::from_utf8(writer.into_inner()).expect("valid UTF-8")
}
