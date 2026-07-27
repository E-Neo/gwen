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

    writer
        .write_event(Event::Empty(BytesStart::new("a:lstStyle")))
        .ok();

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

fn write_paragraph(p: &ParagraphDto, writer: &mut Writer<Vec<u8>>) {
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

fn write_run(r: &RunDto, writer: &mut Writer<Vec<u8>>) {
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

fn write_rpr(font: &FontDto, writer: &mut Writer<Vec<u8>>) {
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

pub fn chart_part_to_xml(chart: &ChartDto) -> String {
    let mut xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"
              xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
              xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <c:chart>
    <c:plotArea>
      <c:layout/>
"#,
    );
    let ct = chart.chart_type.as_deref().unwrap_or("barChart");
    xml.push_str(&format!("      <c:{ct}>\n"));
    if ct == "barChart" {
        xml.push_str("        <c:barDir val=\"col\"/>\n");
        xml.push_str("        <c:grouping val=\"clustered\"/>\n");
    }
    for (i, ser) in chart.series.iter().enumerate() {
        xml.push_str("        <c:ser>\n");
        xml.push_str(&format!("          <c:idx val=\"{i}\"/>\n"));
        xml.push_str(&format!("          <c:order val=\"{i}\"/>\n"));
        if ser.name.is_some() {
            xml.push_str("          <c:tx>\n");
            xml.push_str("            <c:strRef>\n");
            xml.push_str(&format!("              <c:f>Sheet1!$A${}</c:f>\n", i + 1));
            xml.push_str("            </c:strRef>\n");
            xml.push_str("          </c:tx>\n");
        }
        xml.push_str("          <c:cat>\n");
        xml.push_str("            <c:strLit>\n");
        xml.push_str(&format!(
            "              <c:ptCount val=\"{}\"/>\n",
            ser.categories.len()
        ));
        for (j, cat) in ser.categories.iter().enumerate() {
            xml.push_str(&format!(
                "              <c:pt index=\"{j}\"><c:v>{cat}</c:v></c:pt>\n"
            ));
        }
        xml.push_str("            </c:strLit>\n");
        xml.push_str("          </c:cat>\n");
        xml.push_str("          <c:val>\n");
        xml.push_str("            <c:numLit>\n");
        xml.push_str(&format!(
            "              <c:ptCount val=\"{}\"/>\n",
            ser.values.len()
        ));
        for (j, val) in ser.values.iter().enumerate() {
            xml.push_str(&format!(
                "              <c:pt index=\"{j}\"><c:v>{val}</c:v></c:pt>\n"
            ));
        }
        xml.push_str("            </c:numLit>\n");
        xml.push_str("          </c:val>\n");
        xml.push_str("        </c:ser>\n");
    }
    xml.push_str(&format!("      </c:{ct}>\n"));
    xml.push_str(
        "    </c:plotArea>
    <c:plotVisOnly val=\"1\"/>
  </c:chart>
</c:chartSpace>",
    );
    xml
}
