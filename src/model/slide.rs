use std::collections::HashMap;

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::dto::*;
use crate::error::{AppError, AppResult};

fn fresh_run() -> RunDto {
    RunDto {
        text: String::new(),
        font: None,
        hyperlink: None,
    }
}

fn fresh_para() -> ParagraphDto {
    ParagraphDto {
        runs: Vec::new(),
        alignment: None,
        level: Some(0),
        line_spacing: None,
        space_before: None,
        space_after: None,
        font: None,
    }
}

fn fresh_font() -> FontDto {
    FontDto {
        name: None,
        size: None,
        bold: None,
        italic: None,
        underline: None,
        color: None,
    }
}

fn parse_placeholder_type(raw: &str) -> Option<PlaceholderType> {
    match raw {
        "title" => Some(PlaceholderType::Title),
        "body" => Some(PlaceholderType::Body),
        "ctrTitle" => Some(PlaceholderType::CenterTitle),
        "subTitle" => Some(PlaceholderType::SubTitle),
        "obj" => Some(PlaceholderType::Object),
        "chart" => Some(PlaceholderType::Chart),
        "tbl" => Some(PlaceholderType::Table),
        "clipArt" => Some(PlaceholderType::ClipArt),
        "dgm" => Some(PlaceholderType::Diagram),
        "media" => Some(PlaceholderType::Media),
        "sldImg" => Some(PlaceholderType::SlideImage),
        "sldNum" => Some(PlaceholderType::SlideNumber),
        "ftr" => Some(PlaceholderType::Footer),
        "hdr" => Some(PlaceholderType::Header),
        "dt" => Some(PlaceholderType::DateTime),
        "verticalObj" => Some(PlaceholderType::VerticalObject),
        "verticalTitle" => Some(PlaceholderType::VerticalTitle),
        "verticalBody" => Some(PlaceholderType::VerticalBody),
        _ => None,
    }
}

fn parse_alignment(raw: &str) -> Option<Alignment> {
    match raw {
        "l" => Some(Alignment::Left),
        "ctr" => Some(Alignment::Center),
        "r" => Some(Alignment::Right),
        "just" => Some(Alignment::Justify),
        "dist" => Some(Alignment::Distribute),
        "thaiDist" => Some(Alignment::ThaiDistribute),
        "justLow" => Some(Alignment::JustifiedLow),
        _ => None,
    }
}

fn parse_anchor(raw: &str) -> Option<VerticalAnchor> {
    match raw {
        "t" => Some(VerticalAnchor::Top),
        "ctr" => Some(VerticalAnchor::Middle),
        "b" => Some(VerticalAnchor::Bottom),
        "just" => Some(VerticalAnchor::Justified),
        "dist" => Some(VerticalAnchor::Distributed),
        _ => None,
    }
}

pub fn parse_slide_shapes(
    data: &[u8],
    image_map: &HashMap<String, String>,
) -> AppResult<Vec<ShapeDto>> {
    let mut reader = Reader::from_reader(data);
    reader.config_mut().trim_text(true);
    let mut shapes = Vec::new();
    let mut buf = Vec::new();

    let mut current_shape: Option<ShapeDto> = None;
    let mut in_text_frame = false;
    let mut in_paragraph = false;
    let mut in_run = false;
    let mut text_buf = String::new();
    let mut in_xfrm = false;
    let mut is_textbox = false;

    let mut in_run_props = false;
    let mut run_font: Option<FontDto> = None;
    let mut in_solid_fill = false;
    let mut in_end_para_rpr = false;

    let mut in_para_props = false;
    let mut in_ln_spc = false;
    let mut in_spc_bef = false;
    let mut in_spc_aft = false;

    let mut in_body_pr = false;

    let mut body_pr_auto_size: Option<MsoAutoSize> = None;
    let mut body_pr_word_wrap: Option<bool> = None;
    let mut body_pr_anchor: Option<VerticalAnchor> = None;
    let mut body_pr_margin_l: Option<i64> = None;
    let mut body_pr_margin_r: Option<i64> = None;
    let mut body_pr_margin_t: Option<i64> = None;
    let mut body_pr_margin_b: Option<i64> = None;

    let mut run = fresh_run();
    let mut para = fresh_para();
    let mut paragraphs: Vec<ParagraphDto> = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let ename = e.name();
                let tag = ename.as_ref();
                match tag {
                    b"p:sp" | b"p:pic" | b"p:cxnSp" | b"p:grpSp" | b"p:graphicFrame" => {
                        let st = match tag {
                            b"p:pic" => ShapeType::Picture,
                            b"p:cxnSp" => ShapeType::Line,
                            b"p:grpSp" => ShapeType::Group,
                            b"p:graphicFrame" => ShapeType::Chart,
                            _ => ShapeType::AutoShape,
                        };
                        current_shape = Some(ShapeDto {
                            shape_id: 0,
                            name: None,
                            shape_type: st,
                            left: None,
                            top: None,
                            width: None,
                            height: None,
                            rotation: None,
                            is_placeholder: false,
                            has_text_frame: false,
                            placeholder_format: None,
                            auto_shape_type: None,
                            text_frame: None,
                            image: None,
                            shapes: None,
                        });
                        is_textbox = false;
                        in_text_frame = false;
                        in_paragraph = false;
                        in_run = false;
                        text_buf.clear();
                        run = fresh_run();
                        para = fresh_para();
                        paragraphs = Vec::new();
                        in_xfrm = false;
                    }
                    b"p:ph" => {
                        if let Some(ref mut shape) = current_shape {
                            let mut idx = 0i32;
                            let mut ph_type = None;
                            let mut sz = None;
                            for a in e.attributes().flatten() {
                                match a.key.as_ref() {
                                    b"idx" => {
                                        idx = String::from_utf8_lossy(&a.value).parse().unwrap_or(0)
                                    }
                                    b"type" => {
                                        ph_type = String::from_utf8_lossy(&a.value)
                                            .parse::<String>()
                                            .ok()
                                            .and_then(|s| parse_placeholder_type(&s));
                                    }
                                    b"sz" => {
                                        sz = Some(String::from_utf8_lossy(&a.value).to_string())
                                    }
                                    _ => {}
                                }
                            }
                            shape.is_placeholder = true;
                            shape.shape_type = ShapeType::Placeholder;
                            shape.placeholder_format =
                                Some(PlaceholderFormatDto { idx, ph_type, sz });
                        }
                    }
                    b"p:cNvPr" => {
                        if let Some(ref mut shape) = current_shape {
                            for a in e.attributes().flatten() {
                                match a.key.as_ref() {
                                    b"id" => {
                                        shape.shape_id =
                                            String::from_utf8_lossy(&a.value).parse().unwrap_or(0);
                                    }
                                    b"name" => {
                                        shape.name =
                                            Some(String::from_utf8_lossy(&a.value).to_string());
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    b"p:cNvSpPr" => {
                        for a in e.attributes().flatten() {
                            if a.key.as_ref() == b"txBox" && a.value.as_ref() == b"1" {
                                is_textbox = true;
                            }
                        }
                    }
                    b"a:prstGeom" => {
                        if let Some(ref mut shape) = current_shape {
                            for a in e.attributes().flatten() {
                                if a.key.as_ref() == b"prst" {
                                    shape.auto_shape_type =
                                        Some(String::from_utf8_lossy(&a.value).to_string());
                                }
                            }
                        }
                    }
                    b"a:xfrm" | b"p:xfrm" => {
                        in_xfrm = true;
                        if let Some(ref mut shape) = current_shape {
                            for a in e.attributes().flatten() {
                                if a.key.as_ref() == b"rot" {
                                    let raw = String::from_utf8_lossy(&a.value);
                                    if let Ok(v) = raw.parse::<f64>() {
                                        shape.rotation = Some(v / 60000.0);
                                    }
                                }
                            }
                        }
                    }
                    b"a:off" => {
                        if in_xfrm && let Some(ref mut shape) = current_shape {
                            for a in e.attributes().flatten() {
                                match a.key.as_ref() {
                                    b"x" => {
                                        shape.left = String::from_utf8_lossy(&a.value).parse().ok()
                                    }
                                    b"y" => {
                                        shape.top = String::from_utf8_lossy(&a.value).parse().ok()
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    b"a:ext" => {
                        if in_xfrm && let Some(ref mut shape) = current_shape {
                            for a in e.attributes().flatten() {
                                match a.key.as_ref() {
                                    b"cx" => {
                                        shape.width = String::from_utf8_lossy(&a.value).parse().ok()
                                    }
                                    b"cy" => {
                                        shape.height =
                                            String::from_utf8_lossy(&a.value).parse().ok()
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    b"a:blip" => {
                        if let Some(ref mut shape) = current_shape {
                            for a in e.attributes().flatten() {
                                if a.key.as_ref() == b"r:embed"
                                    || a.key.as_ref().ends_with(b"embed")
                                {
                                    let r_id = String::from_utf8_lossy(&a.value).to_string();
                                    shape.image = image_map.get(&r_id).cloned();
                                }
                            }
                        }
                    }
                    b"p:txBody" => {
                        if let Some(ref mut shape) = current_shape {
                            shape.has_text_frame = true;
                            if matches!(shape.shape_type, ShapeType::AutoShape) && is_textbox {
                                shape.shape_type = ShapeType::TextBox;
                            }
                        }
                        in_text_frame = true;
                        paragraphs = Vec::new();
                    }
                    b"a:bodyPr" if in_text_frame => {
                        in_body_pr = true;
                        for a in e.attributes().flatten() {
                            match a.key.as_ref() {
                                b"wrap" => {
                                    body_pr_word_wrap = Some(
                                        String::from_utf8_lossy(&a.value) == "1"
                                            || String::from_utf8_lossy(&a.value)
                                                .to_lowercase()
                                                .contains("sq"),
                                    );
                                }
                                b"anchor" => {
                                    body_pr_anchor = String::from_utf8_lossy(&a.value)
                                        .parse::<String>()
                                        .ok()
                                        .and_then(|s| parse_anchor(&s));
                                }
                                b"lIns" => {
                                    body_pr_margin_l =
                                        String::from_utf8_lossy(&a.value).parse().ok();
                                }
                                b"rIns" => {
                                    body_pr_margin_r =
                                        String::from_utf8_lossy(&a.value).parse().ok();
                                }
                                b"tIns" => {
                                    body_pr_margin_t =
                                        String::from_utf8_lossy(&a.value).parse().ok();
                                }
                                b"bIns" => {
                                    body_pr_margin_b =
                                        String::from_utf8_lossy(&a.value).parse().ok();
                                }
                                _ => {}
                            }
                        }
                    }
                    b"a:spAutoFit" if in_body_pr => {
                        body_pr_auto_size = Some(MsoAutoSize::TextToFitShape);
                    }
                    b"a:normAutofit" if in_body_pr => {
                        body_pr_auto_size = Some(MsoAutoSize::ShapeToFitText);
                    }
                    b"a:noAutofit" if in_body_pr => {
                        body_pr_auto_size = Some(MsoAutoSize::None);
                    }
                    b"a:p" if in_text_frame => {
                        in_paragraph = true;
                        para = fresh_para();
                    }
                    b"a:pPr" if in_paragraph => {
                        in_para_props = true;
                        for a in e.attributes().flatten() {
                            match a.key.as_ref() {
                                b"lvl" => {
                                    para.level = String::from_utf8_lossy(&a.value).parse().ok()
                                }
                                b"algn" => {
                                    para.alignment = String::from_utf8_lossy(&a.value)
                                        .parse::<String>()
                                        .ok()
                                        .and_then(|s| parse_alignment(&s));
                                }
                                _ => {}
                            }
                        }
                    }
                    b"a:lnSpc" if in_para_props => {
                        in_ln_spc = true;
                    }
                    b"a:spcBef" if in_para_props => {
                        in_spc_bef = true;
                    }
                    b"a:spcAft" if in_para_props => {
                        in_spc_aft = true;
                    }
                    b"a:r" if in_paragraph => {
                        in_run = true;
                        run = fresh_run();
                    }
                    b"a:hlinkClick" if in_run => {
                        let address: Option<String> = None;
                        let mut tooltip: Option<String> = None;
                        let mut r_id: Option<String> = None;
                        for a in e.attributes().flatten() {
                            match a.key.as_ref() {
                                b"r:id" => {
                                    r_id = Some(String::from_utf8_lossy(&a.value).to_string());
                                }
                                b"tooltip" => {
                                    tooltip = Some(String::from_utf8_lossy(&a.value).to_string());
                                }
                                _ => {}
                            }
                        }
                        run.hyperlink = Some(HyperlinkDto {
                            address,
                            tooltip,
                            r_id,
                        });
                    }
                    b"a:rPr" if in_run => {
                        run_font = Some(fresh_font());
                        in_run_props = true;
                        for a in e.attributes().flatten() {
                            match a.key.as_ref() {
                                b"sz" => {
                                    if let Some(ref mut font) = run_font {
                                        font.size = String::from_utf8_lossy(&a.value).parse().ok();
                                    }
                                }
                                b"b" | b"i" => {
                                    let v = String::from_utf8_lossy(&a.value) == "1";
                                    if let Some(ref mut font) = run_font {
                                        if a.key.as_ref() == b"b" {
                                            font.bold = Some(v);
                                        } else {
                                            font.italic = Some(v);
                                        }
                                    }
                                }
                                b"u" => {
                                    if let Some(ref mut font) = run_font {
                                        font.underline =
                                            Some(String::from_utf8_lossy(&a.value) != "none");
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    b"a:endParaRPr" if in_paragraph => {
                        run_font = Some(fresh_font());
                        in_run_props = true;
                        in_end_para_rpr = true;
                        for a in e.attributes().flatten() {
                            match a.key.as_ref() {
                                b"sz" => {
                                    if let Some(ref mut font) = run_font {
                                        font.size = String::from_utf8_lossy(&a.value).parse().ok();
                                    }
                                }
                                b"b" | b"i" => {
                                    let v = String::from_utf8_lossy(&a.value) == "1";
                                    if let Some(ref mut font) = run_font {
                                        if a.key.as_ref() == b"b" {
                                            font.bold = Some(v);
                                        } else {
                                            font.italic = Some(v);
                                        }
                                    }
                                }
                                b"u" => {
                                    if let Some(ref mut font) = run_font {
                                        font.underline =
                                            Some(String::from_utf8_lossy(&a.value) != "none");
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    b"a:solidFill" if in_run_props => {
                        in_solid_fill = true;
                    }
                    b"a:latin" if in_run_props => {
                        if let Some(ref mut font) = run_font {
                            for a in e.attributes().flatten() {
                                if a.key.as_ref() == b"typeface" {
                                    font.name = Some(String::from_utf8_lossy(&a.value).to_string());
                                }
                            }
                        }
                    }
                    b"a:ea" if in_run_props => {
                        if let Some(ref mut font) = run_font
                            && font.name.is_none()
                        {
                            for a in e.attributes().flatten() {
                                if a.key.as_ref() == b"typeface" {
                                    font.name = Some(String::from_utf8_lossy(&a.value).to_string());
                                }
                            }
                        }
                    }
                    b"a:cs" if in_run_props => {
                        if let Some(ref mut font) = run_font
                            && font.name.is_none()
                        {
                            for a in e.attributes().flatten() {
                                if a.key.as_ref() == b"typeface" {
                                    font.name = Some(String::from_utf8_lossy(&a.value).to_string());
                                }
                            }
                        }
                    }
                    b"a:sym" if in_run_props => {
                        if let Some(ref mut font) = run_font
                            && font.name.is_none()
                        {
                            for a in e.attributes().flatten() {
                                if a.key.as_ref() == b"typeface" {
                                    font.name = Some(String::from_utf8_lossy(&a.value).to_string());
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) => {
                if in_run && let Ok(t) = String::from_utf8(e.to_vec()) {
                    text_buf.push_str(&t);
                }
            }
            Ok(Event::Empty(ref e)) => {
                let ename = e.name();
                let tag = ename.as_ref();
                match tag {
                    b"p:cNvPr" => {
                        if let Some(ref mut shape) = current_shape {
                            for a in e.attributes().flatten() {
                                match a.key.as_ref() {
                                    b"id" => {
                                        shape.shape_id =
                                            String::from_utf8_lossy(&a.value).parse().unwrap_or(0);
                                    }
                                    b"name" => {
                                        shape.name =
                                            Some(String::from_utf8_lossy(&a.value).to_string());
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    b"p:ph" => {
                        if let Some(ref mut shape) = current_shape {
                            let mut idx = 0i32;
                            let mut ph_type = None;
                            let mut sz = None;
                            for a in e.attributes().flatten() {
                                match a.key.as_ref() {
                                    b"idx" => {
                                        idx = String::from_utf8_lossy(&a.value).parse().unwrap_or(0)
                                    }
                                    b"type" => {
                                        ph_type = String::from_utf8_lossy(&a.value)
                                            .parse::<String>()
                                            .ok()
                                            .and_then(|s| parse_placeholder_type(&s));
                                    }
                                    b"sz" => {
                                        sz = Some(String::from_utf8_lossy(&a.value).to_string())
                                    }
                                    _ => {}
                                }
                            }
                            shape.is_placeholder = true;
                            shape.shape_type = ShapeType::Placeholder;
                            shape.placeholder_format =
                                Some(PlaceholderFormatDto { idx, ph_type, sz });
                        }
                    }
                    b"a:rPr" if in_run => {
                        let mut font = fresh_font();
                        for a in e.attributes().flatten() {
                            match a.key.as_ref() {
                                b"sz" => font.size = String::from_utf8_lossy(&a.value).parse().ok(),
                                b"b" | b"i" => {
                                    let v = String::from_utf8_lossy(&a.value) == "1";
                                    if a.key.as_ref() == b"b" {
                                        font.bold = Some(v);
                                    } else {
                                        font.italic = Some(v);
                                    }
                                }
                                b"u" => {
                                    font.underline =
                                        Some(String::from_utf8_lossy(&a.value) != "none");
                                }
                                _ => {}
                            }
                        }
                        if font.name.is_some()
                            || font.size.is_some()
                            || font.bold.is_some()
                            || font.italic.is_some()
                            || font.underline.is_some()
                            || font.color.is_some()
                        {
                            run.font = Some(font);
                        }
                    }
                    b"a:xfrm" | b"p:xfrm" => {
                        in_xfrm = true;
                        if let Some(ref mut shape) = current_shape {
                            for a in e.attributes().flatten() {
                                if a.key.as_ref() == b"rot" {
                                    let raw = String::from_utf8_lossy(&a.value);
                                    if let Ok(v) = raw.parse::<f64>() {
                                        shape.rotation = Some(v / 60000.0);
                                    }
                                }
                            }
                        }
                    }
                    b"a:off" => {
                        if in_xfrm && let Some(ref mut shape) = current_shape {
                            for a in e.attributes().flatten() {
                                match a.key.as_ref() {
                                    b"x" => {
                                        shape.left = String::from_utf8_lossy(&a.value).parse().ok()
                                    }
                                    b"y" => {
                                        shape.top = String::from_utf8_lossy(&a.value).parse().ok()
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    b"a:ext" => {
                        if in_xfrm && let Some(ref mut shape) = current_shape {
                            for a in e.attributes().flatten() {
                                match a.key.as_ref() {
                                    b"cx" => {
                                        shape.width = String::from_utf8_lossy(&a.value).parse().ok()
                                    }
                                    b"cy" => {
                                        shape.height =
                                            String::from_utf8_lossy(&a.value).parse().ok()
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    b"a:blip" => {
                        if let Some(ref mut shape) = current_shape {
                            for a in e.attributes().flatten() {
                                if a.key.as_ref() == b"r:embed"
                                    || a.key.as_ref().ends_with(b"embed")
                                {
                                    let r_id = String::from_utf8_lossy(&a.value).to_string();
                                    shape.image = image_map.get(&r_id).cloned();
                                }
                            }
                        }
                    }
                    b"a:prstGeom" => {
                        if let Some(ref mut shape) = current_shape {
                            for a in e.attributes().flatten() {
                                if a.key.as_ref() == b"prst" {
                                    shape.auto_shape_type =
                                        Some(String::from_utf8_lossy(&a.value).to_string());
                                }
                            }
                        }
                    }
                    b"p:cNvSpPr" => {
                        for a in e.attributes().flatten() {
                            if a.key.as_ref() == b"txBox" && a.value.as_ref() == b"1" {
                                is_textbox = true;
                            }
                        }
                    }
                    b"a:hlinkClick" if in_run => {
                        let address: Option<String> = None;
                        let mut tooltip: Option<String> = None;
                        let mut r_id: Option<String> = None;
                        for a in e.attributes().flatten() {
                            match a.key.as_ref() {
                                b"r:id" => {
                                    r_id = Some(String::from_utf8_lossy(&a.value).to_string());
                                }
                                b"tooltip" => {
                                    tooltip = Some(String::from_utf8_lossy(&a.value).to_string());
                                }
                                _ => {}
                            }
                        }
                        run.hyperlink = Some(HyperlinkDto {
                            address,
                            tooltip,
                            r_id,
                        });
                    }
                    b"a:endParaRPr" if in_paragraph => {
                        let mut font = fresh_font();
                        for a in e.attributes().flatten() {
                            match a.key.as_ref() {
                                b"sz" => font.size = String::from_utf8_lossy(&a.value).parse().ok(),
                                b"b" | b"i" => {
                                    let v = String::from_utf8_lossy(&a.value) == "1";
                                    if a.key.as_ref() == b"b" {
                                        font.bold = Some(v);
                                    } else {
                                        font.italic = Some(v);
                                    }
                                }
                                b"u" => {
                                    font.underline =
                                        Some(String::from_utf8_lossy(&a.value) != "none");
                                }
                                _ => {}
                            }
                        }
                        if font.name.is_some()
                            || font.size.is_some()
                            || font.bold.is_some()
                            || font.italic.is_some()
                            || font.underline.is_some()
                            || font.color.is_some()
                        {
                            para.font = Some(font);
                        }
                    }
                    b"a:solidFill" if in_run_props => {
                        in_solid_fill = true;
                    }
                    b"a:spAutoFit" if in_body_pr => {
                        body_pr_auto_size = Some(MsoAutoSize::TextToFitShape);
                    }
                    b"a:normAutofit" if in_body_pr => {
                        body_pr_auto_size = Some(MsoAutoSize::ShapeToFitText);
                    }
                    b"a:noAutofit" if in_body_pr => {
                        body_pr_auto_size = Some(MsoAutoSize::None);
                    }
                    b"a:latin" if in_run_props => {
                        if let Some(ref mut font) = run_font {
                            for a in e.attributes().flatten() {
                                if a.key.as_ref() == b"typeface" {
                                    font.name = Some(String::from_utf8_lossy(&a.value).to_string());
                                }
                            }
                        }
                    }
                    b"a:ea" if in_run_props => {
                        if let Some(ref mut font) = run_font
                            && font.name.is_none()
                        {
                            for a in e.attributes().flatten() {
                                if a.key.as_ref() == b"typeface" {
                                    font.name = Some(String::from_utf8_lossy(&a.value).to_string());
                                }
                            }
                        }
                    }
                    b"a:cs" if in_run_props => {
                        if let Some(ref mut font) = run_font
                            && font.name.is_none()
                        {
                            for a in e.attributes().flatten() {
                                if a.key.as_ref() == b"typeface" {
                                    font.name = Some(String::from_utf8_lossy(&a.value).to_string());
                                }
                            }
                        }
                    }
                    b"a:sym" if in_run_props => {
                        if let Some(ref mut font) = run_font
                            && font.name.is_none()
                        {
                            for a in e.attributes().flatten() {
                                if a.key.as_ref() == b"typeface" {
                                    font.name = Some(String::from_utf8_lossy(&a.value).to_string());
                                }
                            }
                        }
                    }
                    b"a:schemeClr" if in_solid_fill => {
                        if let Some(ref mut font) = run_font {
                            for a in e.attributes().flatten() {
                                if a.key.as_ref() == b"val" {
                                    font.color = Some(ColorFormatDto {
                                        color_type: Some(ColorType::Scheme),
                                        rgb: None,
                                        theme_color: Some(
                                            String::from_utf8_lossy(&a.value).to_string(),
                                        ),
                                        brightness: None,
                                    });
                                }
                            }
                        }
                    }
                    b"a:srgbClr" if in_solid_fill => {
                        if let Some(ref mut font) = run_font {
                            for a in e.attributes().flatten() {
                                if a.key.as_ref() == b"val" {
                                    font.color = Some(ColorFormatDto {
                                        color_type: Some(ColorType::Rgb),
                                        rgb: Some(String::from_utf8_lossy(&a.value).to_string()),
                                        theme_color: None,
                                        brightness: None,
                                    });
                                }
                            }
                        }
                    }
                    b"a:spcPts" if in_ln_spc => {
                        for a in e.attributes().flatten() {
                            if a.key.as_ref() == b"val" {
                                let raw: f64 =
                                    String::from_utf8_lossy(&a.value).parse().unwrap_or(0.0);
                                para.line_spacing = Some(raw / 100.0);
                            }
                        }
                    }
                    b"a:spcPct" if in_ln_spc => {
                        for a in e.attributes().flatten() {
                            if a.key.as_ref() == b"val" {
                                let raw: f64 =
                                    String::from_utf8_lossy(&a.value).parse().unwrap_or(0.0);
                                para.line_spacing = Some(raw / 100000.0);
                            }
                        }
                    }
                    b"a:spcPts" if in_spc_bef => {
                        for a in e.attributes().flatten() {
                            if a.key.as_ref() == b"val" {
                                para.space_before = String::from_utf8_lossy(&a.value).parse().ok();
                            }
                        }
                    }
                    b"a:spcPts" if in_spc_aft => {
                        for a in e.attributes().flatten() {
                            if a.key.as_ref() == b"val" {
                                para.space_after = String::from_utf8_lossy(&a.value).parse().ok();
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let ename = e.name();
                let tag = ename.as_ref();
                match tag {
                    b"p:sp" | b"p:pic" | b"p:cxnSp" | b"p:grpSp" | b"p:graphicFrame" => {
                        if let Some(mut shape) = current_shape.take() {
                            if !paragraphs.is_empty() {
                                shape.text_frame = Some(TextFrameDto {
                                    paragraphs,
                                    auto_size: body_pr_auto_size.take(),
                                    word_wrap: body_pr_word_wrap.take(),
                                    vertical_anchor: body_pr_anchor.take(),
                                    margin_left: body_pr_margin_l.take(),
                                    margin_right: body_pr_margin_r.take(),
                                    margin_top: body_pr_margin_t.take(),
                                    margin_bottom: body_pr_margin_b.take(),
                                });
                            } else {
                                body_pr_auto_size = None;
                                body_pr_word_wrap = None;
                                body_pr_anchor = None;
                                body_pr_margin_l = None;
                                body_pr_margin_r = None;
                                body_pr_margin_t = None;
                                body_pr_margin_b = None;
                            }
                            paragraphs = Vec::new();
                            shapes.push(shape);
                        }
                        in_text_frame = false;
                        in_paragraph = false;
                        in_run = false;
                        in_run_props = false;
                        run_font = None;
                        in_solid_fill = false;
                        in_end_para_rpr = false;
                        in_para_props = false;
                        in_ln_spc = false;
                        in_spc_bef = false;
                        in_spc_aft = false;
                        in_body_pr = false;
                    }
                    b"a:xfrm" | b"p:xfrm" => {
                        in_xfrm = false;
                    }
                    b"a:pPr" if in_para_props => {
                        in_para_props = false;
                    }
                    b"a:lnSpc" if in_ln_spc => {
                        in_ln_spc = false;
                    }
                    b"a:spcBef" if in_spc_bef => {
                        in_spc_bef = false;
                    }
                    b"a:spcAft" if in_spc_aft => {
                        in_spc_aft = false;
                    }
                    b"a:bodyPr" if in_body_pr => {
                        in_body_pr = false;
                    }
                    b"a:r" if in_run => {
                        run.text = text_buf.trim().to_string();
                        para.runs.push(run);
                        run = fresh_run();
                        text_buf.clear();
                        in_run = false;
                    }
                    b"a:rPr" if in_run_props => {
                        if let Some(font) = run_font.take()
                            && (font.name.is_some()
                                || font.size.is_some()
                                || font.bold.is_some()
                                || font.italic.is_some()
                                || font.underline.is_some()
                                || font.color.is_some())
                        {
                            run.font = Some(font);
                        }
                        in_run_props = false;
                        in_solid_fill = false;
                    }
                    b"a:endParaRPr" if in_end_para_rpr => {
                        if let Some(font) = run_font.take() {
                            para.font = Some(font);
                        }
                        in_run_props = false;
                        in_solid_fill = false;
                        in_end_para_rpr = false;
                    }
                    b"a:solidFill" if in_solid_fill => {
                        in_solid_fill = false;
                    }
                    b"a:p" if in_paragraph => {
                        if in_run {
                            run.text = text_buf.trim().to_string();
                            para.runs.push(run);
                            run = fresh_run();
                            text_buf.clear();
                            in_run = false;
                        }
                        paragraphs.push(para);
                        para = fresh_para();
                        in_paragraph = false;
                    }
                    b"p:txBody" => {
                        if in_paragraph {
                            if in_run {
                                run.text = text_buf.trim().to_string();
                                para.runs.push(run);
                                run = fresh_run();
                                text_buf.clear();
                                in_run = false;
                            }
                            paragraphs.push(para);
                            para = fresh_para();
                        }
                        in_text_frame = false;
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(AppError::Xml(e)),
            _ => {}
        }
    }

    Ok(shapes)
}
