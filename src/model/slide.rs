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

fn fresh_shape(shape_type: ShapeType) -> ShapeDto {
    ShapeDto {
        shape_id: 0,
        name: None,
        shape_type,
        left: None,
        top: None,
        width: None,
        height: None,
        rotation: None,
        ch_off_x: None,
        ch_off_y: None,
        ch_ext_cx: None,
        ch_ext_cy: None,
        is_placeholder: false,
        has_text_frame: false,
        fill: None,
        outline: None,
        placeholder_format: None,
        auto_shape_type: None,
        text_frame: None,
        image: None,
        table: None,
        chart: None,
        shapes: None,
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

/// Resolve the shape that property handlers should mutate: the innermost open
/// shape (`current_shape`) if one is being parsed, otherwise the innermost open
/// group (so a group's own `nvGrpSpPr`/`grpSpPr` properties land on the group).
fn shape_target<'a>(
    current_shape: &'a mut Option<ShapeDto>,
    group_stack: &'a mut [ShapeDto],
) -> Option<&'a mut ShapeDto> {
    if current_shape.is_some() {
        current_shape.as_mut()
    } else {
        group_stack.last_mut()
    }
}pub fn parse_slide_shapes(
    data: &[u8],
    image_map: &HashMap<String, String>,
) -> AppResult<Vec<ShapeDto>> {
    let mut reader = Reader::from_reader(data);
    reader.config_mut().trim_text(true);
    let mut shapes = Vec::new();
    let mut buf = Vec::new();

    let mut current_shape: Option<ShapeDto> = None;
    let mut group_stack: Vec<ShapeDto> = Vec::new();
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

    let mut in_sp_pr = false;
    let mut in_shape_fill = false;
    let mut in_shape_ln = false;
    let mut in_ln_fill = false;
    let mut shape_fill_type: Option<FillType> = None;
    let mut shape_fill_color: Option<ColorFormatDto> = None;
    let mut ln_width: Option<i64> = None;
    let mut ln_cap: Option<LineCap> = None;
    let mut ln_compound: Option<CompoundLine> = None;
    let mut ln_dash: Option<LineDash> = None;
    let mut ln_fill_type: Option<FillType> = None;
    let mut ln_fill_color: Option<ColorFormatDto> = None;

    let mut run = fresh_run();
    let mut para = fresh_para();
    let mut paragraphs: Vec<ParagraphDto> = Vec::new();

    // Table/chart parsing state
    let mut in_table = false;
    let mut in_tr = false;
    let mut in_tc = false;
    let mut in_cell_text = false;
    let mut in_graphic_data = false;
    let mut table_grid: Vec<GridColDto> = Vec::new();
    let mut current_cells: Vec<TableCellDto> = Vec::new();
    let mut table_rows: Vec<TableRowDto> = Vec::new();
    let mut tc_row_span: Option<u32> = None;
    let mut tc_grid_span: Option<u32> = None;
    let mut tc_h_merge: Option<bool> = None;
    let mut tc_v_merge: Option<bool> = None;
    let mut current_row_height: Option<i64> = None;
    let mut cell_paragraphs: Vec<ParagraphDto> = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let ename = e.name();
                let tag = ename.as_ref();
                match tag {
                    b"p:grpSp" => {
                        group_stack.push(fresh_shape(ShapeType::Group));
                        is_textbox = false;
                        in_text_frame = false;
                        in_paragraph = false;
                        in_run = false;
                        text_buf.clear();
                        run = fresh_run();
                        para = fresh_para();
                        paragraphs = Vec::new();
                        in_xfrm = false;
                        in_table = false;
                        in_tr = false;
                        in_tc = false;
                        in_cell_text = false;
                        in_graphic_data = false;
                        table_grid.clear();
                        current_cells.clear();
                        table_rows.clear();
                    }
                    b"p:sp" | b"p:pic" | b"p:cxnSp" | b"p:graphicFrame" => {
                        let mut st = match tag {
                            b"p:pic" => ShapeType::Picture,
                            b"p:cxnSp" => ShapeType::Line,
                            b"p:graphicFrame" => ShapeType::Chart,
                            _ => ShapeType::AutoShape,
                        };
                        let is_gf = tag == b"p:graphicFrame";
                        if is_gf {
                            st = ShapeType::AutoShape; // will be corrected by a:graphicData handler
                        }
                        current_shape = Some(fresh_shape(st));
                        is_textbox = false;
                        in_text_frame = false;
                        in_paragraph = false;
                        in_run = false;
                        text_buf.clear();
                        run = fresh_run();
                        para = fresh_para();
                        paragraphs = Vec::new();
                        in_xfrm = false;
                        in_table = false;
                        in_tr = false;
                        in_tc = false;
                        in_cell_text = false;
                        in_graphic_data = false;
                        table_grid.clear();
                        current_cells.clear();
                        table_rows.clear();
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
                        if let Some(ref mut shape) =
                            shape_target(&mut current_shape, &mut group_stack)
                        {
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
                    b"p:spPr" => {
                        in_sp_pr = true;
                    }
                    b"a:noFill" if in_sp_pr && !in_shape_ln => {
                        shape_fill_type = Some(FillType::NoFill);
                    }
                    b"a:solidFill" if in_sp_pr && !in_shape_ln => {
                        in_shape_fill = true;
                    }
                    b"a:solidFill" if in_shape_ln => {
                        in_ln_fill = true;
                    }
                    b"a:noFill" if in_shape_ln => {
                        ln_fill_type = Some(FillType::NoFill);
                    }
                    b"a:ln" if in_sp_pr => {
                        in_shape_ln = true;
                        for a in e.attributes().flatten() {
                            match a.key.as_ref() {
                                b"w" => {
                                    ln_width = String::from_utf8_lossy(&a.value).parse().ok();
                                }
                                b"cap" => {
                                    ln_cap = match String::from_utf8_lossy(&a.value).as_ref() {
                                        "rnd" => Some(LineCap::Rnd),
                                        "sq" => Some(LineCap::Sq),
                                        "flat" => Some(LineCap::Flat),
                                        _ => None,
                                    };
                                }
                                b"cmpd" => {
                                    ln_compound = match String::from_utf8_lossy(&a.value).as_ref() {
                                        "sng" => Some(CompoundLine::Sng),
                                        "dbl" => Some(CompoundLine::Dbl),
                                        "thickThin" => Some(CompoundLine::ThickThin),
                                        "thinThick" => Some(CompoundLine::ThinThick),
                                        "tri" => Some(CompoundLine::Tri),
                                        _ => None,
                                    };
                                }
                                _ => {}
                            }
                        }
                    }
                    b"a:prstDash" if in_shape_ln => {
                        for a in e.attributes().flatten() {
                            if a.key.as_ref() == b"val" {
                                ln_dash = match String::from_utf8_lossy(&a.value).as_ref() {
                                    "solid" => Some(LineDash::Solid),
                                    "dot" => Some(LineDash::Dot),
                                    "dash" => Some(LineDash::Dash),
                                    "lgDash" => Some(LineDash::LgDash),
                                    "dashDot" => Some(LineDash::DashDot),
                                    "lgDashDot" => Some(LineDash::LgDashDot),
                                    "lgDashDotDot" => Some(LineDash::LgDashDotDot),
                                    "sysDash" => Some(LineDash::SysDash),
                                    "sysDot" => Some(LineDash::SysDot),
                                    "sysDashDot" => Some(LineDash::SysDashDot),
                                    "sysDashDotDot" => Some(LineDash::SysDashDotDot),
                                    _ => None,
                                };
                            }
                        }
                    }
                    b"a:xfrm" | b"p:xfrm" => {
                        in_xfrm = true;
                        if let Some(ref mut shape) =
                            shape_target(&mut current_shape, &mut group_stack)
                        {
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
                        if in_xfrm
                            && let Some(ref mut shape) =
                                shape_target(&mut current_shape, &mut group_stack)
                        {
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
                        if in_xfrm
                            && let Some(ref mut shape) =
                                shape_target(&mut current_shape, &mut group_stack)
                        {
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
                    b"a:chOff" => {
                        if in_xfrm
                            && let Some(ref mut shape) =
                                shape_target(&mut current_shape, &mut group_stack)
                        {
                            for a in e.attributes().flatten() {
                                match a.key.as_ref() {
                                    b"x" => {
                                        shape.ch_off_x =
                                            String::from_utf8_lossy(&a.value).parse().ok()
                                    }
                                    b"y" => {
                                        shape.ch_off_y =
                                            String::from_utf8_lossy(&a.value).parse().ok()
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    b"a:chExt" => {
                        if in_xfrm
                            && let Some(ref mut shape) =
                                shape_target(&mut current_shape, &mut group_stack)
                        {
                            for a in e.attributes().flatten() {
                                match a.key.as_ref() {
                                    b"cx" => {
                                        shape.ch_ext_cx =
                                            String::from_utf8_lossy(&a.value).parse().ok()
                                    }
                                    b"cy" => {
                                        shape.ch_ext_cy =
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
                    b"a:srgbClr" if in_shape_fill => {
                        for a in e.attributes().flatten() {
                            if a.key.as_ref() == b"val" {
                                shape_fill_color = Some(ColorFormatDto {
                                    color_type: Some(ColorType::Rgb),
                                    rgb: Some(String::from_utf8_lossy(&a.value).to_string()),
                                    theme_color: None,
                                    brightness: None,
                                });
                            }
                        }
                    }
                    b"a:schemeClr" if in_shape_fill => {
                        for a in e.attributes().flatten() {
                            if a.key.as_ref() == b"val" {
                                shape_fill_color = Some(ColorFormatDto {
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
                    b"a:srgbClr" if in_ln_fill => {
                        for a in e.attributes().flatten() {
                            if a.key.as_ref() == b"val" {
                                ln_fill_color = Some(ColorFormatDto {
                                    color_type: Some(ColorType::Rgb),
                                    rgb: Some(String::from_utf8_lossy(&a.value).to_string()),
                                    theme_color: None,
                                    brightness: None,
                                });
                            }
                        }
                    }
                    b"a:schemeClr" if in_ln_fill => {
                        for a in e.attributes().flatten() {
                            if a.key.as_ref() == b"val" {
                                ln_fill_color = Some(ColorFormatDto {
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
                    b"a:p" if in_text_frame || in_cell_text => {
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
                    b"a:graphicData" if current_shape.is_some() => {
                        in_graphic_data = true;
                        for a in e.attributes().flatten() {
                            if a.key.as_ref() == b"uri" {
                                let uri = String::from_utf8_lossy(&a.value);
                                if uri.contains("table") {
                                    in_table = true;
                                    if let Some(ref mut shape) = current_shape {
                                        shape.shape_type = ShapeType::Table;
                                    }
                                } else if uri.contains("chart")
                                    && let Some(ref mut shape) = current_shape
                                {
                                    shape.shape_type = ShapeType::Chart;
                                }
                            }
                        }
                    }
                    b"a:tbl" if in_table => {
                        table_grid.clear();
                        table_rows.clear();
                    }
                    b"a:tblGrid" if in_table => {}
                    b"a:gridCol" if in_table => {
                        for a in e.attributes().flatten() {
                            if a.key.as_ref() == b"w" {
                                let w: i64 = String::from_utf8_lossy(&a.value).parse().unwrap_or(0);
                                table_grid.push(GridColDto { width: w });
                            }
                        }
                    }
                    b"a:tr" if in_table => {
                        in_tr = true;
                        current_cells.clear();
                        current_row_height = None;
                        for a in e.attributes().flatten() {
                            if a.key.as_ref() == b"h" {
                                current_row_height = String::from_utf8_lossy(&a.value).parse().ok();
                            }
                        }
                    }
                    b"a:tc" if in_tr => {
                        in_tc = true;
                        tc_row_span = None;
                        tc_grid_span = None;
                        tc_h_merge = None;
                        tc_v_merge = None;
                        cell_paragraphs.clear();
                        for a in e.attributes().flatten() {
                            match a.key.as_ref() {
                                b"rowSpan" => {
                                    tc_row_span = String::from_utf8_lossy(&a.value).parse().ok()
                                }
                                b"gridSpan" => {
                                    tc_grid_span = String::from_utf8_lossy(&a.value).parse().ok()
                                }
                                b"hMerge" => {
                                    tc_h_merge = Some(String::from_utf8_lossy(&a.value) == "1")
                                }
                                b"vMerge" => {
                                    tc_v_merge = Some(String::from_utf8_lossy(&a.value) == "1")
                                }
                                _ => {}
                            }
                        }
                    }
                    b"a:txBody" if in_tc => {
                        in_cell_text = true;
                        cell_paragraphs.clear();
                    }
                    b"a:bodyPr" if in_cell_text => {
                        in_body_pr = true;
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
                        if let Some(ref mut shape) =
                            shape_target(&mut current_shape, &mut group_stack)
                        {
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
                        if let Some(ref mut shape) =
                            shape_target(&mut current_shape, &mut group_stack)
                        {
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
                        if in_xfrm
                            && let Some(ref mut shape) =
                                shape_target(&mut current_shape, &mut group_stack)
                        {
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
                        if in_xfrm
                            && let Some(ref mut shape) =
                                shape_target(&mut current_shape, &mut group_stack)
                        {
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
                    b"a:chOff" => {
                        if in_xfrm
                            && let Some(ref mut shape) =
                                shape_target(&mut current_shape, &mut group_stack)
                        {
                            for a in e.attributes().flatten() {
                                match a.key.as_ref() {
                                    b"x" => {
                                        shape.ch_off_x =
                                            String::from_utf8_lossy(&a.value).parse().ok()
                                    }
                                    b"y" => {
                                        shape.ch_off_y =
                                            String::from_utf8_lossy(&a.value).parse().ok()
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    b"a:chExt" => {
                        if in_xfrm
                            && let Some(ref mut shape) =
                                shape_target(&mut current_shape, &mut group_stack)
                        {
                            for a in e.attributes().flatten() {
                                match a.key.as_ref() {
                                    b"cx" => {
                                        shape.ch_ext_cx =
                                            String::from_utf8_lossy(&a.value).parse().ok()
                                    }
                                    b"cy" => {
                                        shape.ch_ext_cy =
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
                    b"a:noFill" if in_sp_pr && !in_shape_ln => {
                        shape_fill_type = Some(FillType::NoFill);
                    }
                    b"a:solidFill" if in_sp_pr && !in_shape_ln => {
                        in_shape_fill = true;
                    }
                    b"a:solidFill" if in_shape_ln => {
                        in_ln_fill = true;
                    }
                    b"a:noFill" if in_shape_ln => {
                        ln_fill_type = Some(FillType::NoFill);
                    }
                    b"a:ln" if in_sp_pr => {
                        in_shape_ln = true;
                        for a in e.attributes().flatten() {
                            match a.key.as_ref() {
                                b"w" => {
                                    ln_width = String::from_utf8_lossy(&a.value).parse().ok();
                                }
                                b"cap" => {
                                    ln_cap = match String::from_utf8_lossy(&a.value).as_ref() {
                                        "rnd" => Some(LineCap::Rnd),
                                        "sq" => Some(LineCap::Sq),
                                        "flat" => Some(LineCap::Flat),
                                        _ => None,
                                    };
                                }
                                b"cmpd" => {
                                    ln_compound = match String::from_utf8_lossy(&a.value).as_ref() {
                                        "sng" => Some(CompoundLine::Sng),
                                        "dbl" => Some(CompoundLine::Dbl),
                                        "thickThin" => Some(CompoundLine::ThickThin),
                                        "thinThick" => Some(CompoundLine::ThinThick),
                                        "tri" => Some(CompoundLine::Tri),
                                        _ => None,
                                    };
                                }
                                _ => {}
                            }
                        }
                    }
                    b"a:prstDash" if in_shape_ln => {
                        for a in e.attributes().flatten() {
                            if a.key.as_ref() == b"val" {
                                ln_dash = match String::from_utf8_lossy(&a.value).as_ref() {
                                    "solid" => Some(LineDash::Solid),
                                    "dot" => Some(LineDash::Dot),
                                    "dash" => Some(LineDash::Dash),
                                    "lgDash" => Some(LineDash::LgDash),
                                    "dashDot" => Some(LineDash::DashDot),
                                    "lgDashDot" => Some(LineDash::LgDashDot),
                                    "lgDashDotDot" => Some(LineDash::LgDashDotDot),
                                    "sysDash" => Some(LineDash::SysDash),
                                    "sysDot" => Some(LineDash::SysDot),
                                    "sysDashDot" => Some(LineDash::SysDashDot),
                                    "sysDashDotDot" => Some(LineDash::SysDashDotDot),
                                    _ => None,
                                };
                            }
                        }
                    }
                    b"a:srgbClr" if in_shape_fill => {
                        for a in e.attributes().flatten() {
                            if a.key.as_ref() == b"val" {
                                shape_fill_color = Some(ColorFormatDto {
                                    color_type: Some(ColorType::Rgb),
                                    rgb: Some(String::from_utf8_lossy(&a.value).to_string()),
                                    theme_color: None,
                                    brightness: None,
                                });
                            }
                        }
                    }
                    b"a:schemeClr" if in_shape_fill => {
                        for a in e.attributes().flatten() {
                            if a.key.as_ref() == b"val" {
                                shape_fill_color = Some(ColorFormatDto {
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
                    b"a:srgbClr" if in_ln_fill => {
                        for a in e.attributes().flatten() {
                            if a.key.as_ref() == b"val" {
                                ln_fill_color = Some(ColorFormatDto {
                                    color_type: Some(ColorType::Rgb),
                                    rgb: Some(String::from_utf8_lossy(&a.value).to_string()),
                                    theme_color: None,
                                    brightness: None,
                                });
                            }
                        }
                    }
                    b"a:schemeClr" if in_ln_fill => {
                        for a in e.attributes().flatten() {
                            if a.key.as_ref() == b"val" {
                                ln_fill_color = Some(ColorFormatDto {
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
                    b"a:gridCol" if in_table => {
                        for a in e.attributes().flatten() {
                            if a.key.as_ref() == b"w" {
                                let w: i64 = String::from_utf8_lossy(&a.value).parse().unwrap_or(0);
                                table_grid.push(GridColDto { width: w });
                            }
                        }
                    }
                    b"c:chart" if in_graphic_data => {
                        for a in e.attributes().flatten() {
                            if a.key.as_ref() == b"r:id" || a.key.as_ref().ends_with(b":id") {
                                let r_id = String::from_utf8_lossy(&a.value).to_string();
                                if let Some(ref mut shape) = current_shape {
                                    shape.chart = Some(ChartDto {
                                        chart_type: None,
                                        r_id: Some(r_id),
                                        series: Vec::new(),
                                    });
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
                    b"a:txBody" if in_tc => {
                        in_cell_text = true;
                        cell_paragraphs.clear();
                    }
                    b"a:bodyPr" if in_cell_text => {
                        in_body_pr = true;
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let ename = e.name();
                let tag = ename.as_ref();
                match tag {
                    b"p:sp" | b"p:pic" | b"p:cxnSp" | b"p:grpSp" | b"p:graphicFrame" => {
                        if tag == b"p:grpSp" {
                            if let Some(group) = group_stack.pop() {
                                if let Some(parent) = group_stack.last_mut() {
                                    let children = parent.shapes.get_or_insert_with(Vec::new);
                                    children.push(group);
                                } else {
                                    shapes.push(group);
                                }
                            }
                        } else if let Some(mut shape) = current_shape.take() {
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
                            if let Some(parent) = group_stack.last_mut() {
                                parent.shapes.get_or_insert_with(Vec::new).push(shape);
                            } else {
                                shapes.push(shape);
                            }
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
                        in_table = false;
                        in_tr = false;
                        in_tc = false;
                        in_cell_text = false;
                        in_graphic_data = false;
                        table_grid.clear();
                        current_cells.clear();
                        table_rows.clear();
                        in_sp_pr = false;
                        in_shape_fill = false;
                        in_shape_ln = false;
                        in_ln_fill = false;
                        shape_fill_type = None;
                        shape_fill_color = None;
                        ln_width = None;
                        ln_cap = None;
                        ln_compound = None;
                        ln_dash = None;
                        ln_fill_type = None;
                        ln_fill_color = None;
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
                    b"p:spPr" => {
                        if let Some(ref mut shape) = current_shape {
                            if let Some(ft) = shape_fill_type.take() {
                                shape.fill = Some(FillDto {
                                    fill_type: Some(ft),
                                    color: shape_fill_color.take(),
                                    alpha: None,
                                });
                            } else if let Some(color) = shape_fill_color.take() {
                                shape.fill = Some(FillDto {
                                    fill_type: Some(FillType::Solid),
                                    color: Some(color),
                                    alpha: None,
                                });
                            }
                            let ln_fill = ln_fill_type
                                .take()
                                .or_else(|| ln_fill_color.as_ref().map(|_| FillType::Solid));
                            let has_ln = ln_width.is_some()
                                || ln_cap.is_some()
                                || ln_compound.is_some()
                                || ln_dash.is_some()
                                || ln_fill_color.is_some()
                                || ln_fill_type.is_some();
                            if has_ln {
                                shape.outline = Some(OutlineDto {
                                    width: ln_width.take(),
                                    cap: ln_cap.take(),
                                    compound: ln_compound.take(),
                                    dash: ln_dash.take(),
                                    fill: ln_fill.map(|ft| FillDto {
                                        fill_type: Some(ft),
                                        color: ln_fill_color.take(),
                                        alpha: None,
                                    }),
                                });
                            }
                        }
                        in_sp_pr = false;
                        in_shape_ln = false;
                        in_shape_fill = false;
                        in_ln_fill = false;
                        shape_fill_type = None;
                        shape_fill_color = None;
                        ln_width = None;
                        ln_cap = None;
                        ln_compound = None;
                        ln_dash = None;
                        ln_fill_type = None;
                        ln_fill_color = None;
                    }
                    b"a:ln" if in_shape_ln => {
                        in_shape_ln = false;
                        in_ln_fill = false;
                    }
                    b"a:solidFill" if in_shape_fill => {
                        in_shape_fill = false;
                    }
                    b"a:solidFill" if in_ln_fill => {
                        in_ln_fill = false;
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
                        if in_cell_text {
                            cell_paragraphs.push(para);
                        } else {
                            paragraphs.push(para);
                        }
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
                    b"a:txBody" if in_cell_text => {
                        if in_paragraph {
                            if in_run {
                                run.text = text_buf.trim().to_string();
                                para.runs.push(run);
                                run = fresh_run();
                                text_buf.clear();
                                in_run = false;
                            }
                            cell_paragraphs.push(para);
                            para = fresh_para();
                            in_paragraph = false;
                        }
                        in_cell_text = false;
                        in_body_pr = false;
                    }
                    b"a:tc" if in_tc => {
                        let cell = TableCellDto {
                            row_span: tc_row_span.take(),
                            grid_span: tc_grid_span.take(),
                            h_merge: tc_h_merge.take(),
                            v_merge: tc_v_merge.take(),
                            text_frame: if cell_paragraphs.is_empty() {
                                None
                            } else {
                                Some(TextFrameDto {
                                    paragraphs: std::mem::take(&mut cell_paragraphs),
                                    auto_size: None,
                                    word_wrap: None,
                                    vertical_anchor: None,
                                    margin_left: None,
                                    margin_right: None,
                                    margin_top: None,
                                    margin_bottom: None,
                                })
                            },
                        };
                        current_cells.push(cell);
                        in_tc = false;
                    }
                    b"a:tr" if in_tr => {
                        let row = TableRowDto {
                            height: current_row_height.take(),
                            cells: std::mem::take(&mut current_cells),
                        };
                        table_rows.push(row);
                        in_tr = false;
                    }
                    b"a:tbl" if in_table => {
                        let table = TableDto {
                            grid: std::mem::take(&mut table_grid),
                            rows: std::mem::take(&mut table_rows),
                        };
                        if let Some(ref mut shape) = current_shape {
                            shape.table = Some(table);
                        }
                        in_table = false;
                    }
                    b"a:graphicData" if in_graphic_data => {
                        in_graphic_data = false;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_map() -> HashMap<String, String> {
        HashMap::new()
    }

    #[test]
    fn group_children_are_nested() {
        let xml = br#"
        <p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
               xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
          <p:cSld><p:spTree>
            <p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>
            <p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>
            <p:grpSp>
              <p:nvGrpSpPr><p:cNvPr id="2" name="My Group"/><p:cNvGrpSpPr><a:grpSpLocks noGrp="1"/></p:cNvGrpSpPr><p:nvPr/></p:nvGrpSpPr>
              <p:grpSpPr><a:xfrm><a:off x="100" y="200"/><a:ext cx="500" cy="300"/><a:chOff x="0" y="0"/><a:chExt cx="1000" cy="600"/></a:xfrm></p:grpSpPr>
              <p:sp><p:nvSpPr><p:cNvPr id="3" name="Inner 1"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="10" y="20"/><a:ext cx="100" cy="50"/></a:xfrm></p:spPr><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>hi</a:t></a:r></a:p></p:txBody></p:sp>
              <p:sp><p:nvSpPr><p:cNvPr id="4" name="Inner 2"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="200" y="20"/><a:ext cx="100" cy="50"/></a:xfrm></p:spPr></p:sp>
            </p:grpSp>
          </p:spTree></p:cSld>
        </p:sld>
        "#;
        let shapes = parse_slide_shapes(xml, &empty_map()).unwrap();
        assert_eq!(shapes.len(), 1);
        let group = &shapes[0];
        assert_eq!(group.shape_type, ShapeType::Group);
        assert_eq!(group.name.as_deref(), Some("My Group"));
        assert_eq!(group.left, Some(100));
        assert_eq!(group.top, Some(200));
        assert_eq!(group.width, Some(500));
        assert_eq!(group.height, Some(300));
        assert_eq!(group.ch_off_x, Some(0));
        assert_eq!(group.ch_ext_cx, Some(1000));
        assert_eq!(group.ch_ext_cy, Some(600));
        let children = group.shapes.as_ref().unwrap();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].name.as_deref(), Some("Inner 1"));
        assert_eq!(children[0].left, Some(10));
        assert_eq!(children[0].shape_type, ShapeType::AutoShape);
        let text = children[0].text_frame.as_ref().unwrap();
        assert_eq!(text.paragraphs[0].runs[0].text, "hi");
        assert_eq!(children[1].name.as_deref(), Some("Inner 2"));
        assert_eq!(children[1].left, Some(200));
    }

    #[test]
    fn nested_groups_are_preserved() {
        let xml = br#"
        <p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
               xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
          <p:cSld><p:spTree>
            <p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>
            <p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>
            <p:grpSp>
              <p:nvGrpSpPr><p:cNvPr id="2" name="Outer"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>
              <p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="100" cy="100"/><a:chOff x="0" y="0"/><a:chExt cx="100" cy="100"/></a:xfrm></p:grpSpPr>
              <p:grpSp>
                <p:nvGrpSpPr><p:cNvPr id="3" name="Inner"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>
                <p:grpSpPr><a:xfrm><a:off x="5" y="5"/><a:ext cx="50" cy="50"/><a:chOff x="0" y="0"/><a:chExt cx="50" cy="50"/></a:xfrm></p:grpSpPr>
                <p:sp><p:nvSpPr><p:cNvPr id="4" name="Leaf"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="10" cy="10"/></a:xfrm></p:spPr></p:sp>
              </p:grpSp>
            </p:grpSp>
          </p:spTree></p:cSld>
        </p:sld>
        "#;
        let shapes = parse_slide_shapes(xml, &empty_map()).unwrap();
        assert_eq!(shapes.len(), 1);
        let outer = &shapes[0];
        assert_eq!(outer.shape_type, ShapeType::Group);
        assert_eq!(outer.name.as_deref(), Some("Outer"));
        let inner = &outer.shapes.as_ref().unwrap()[0];
        assert_eq!(inner.shape_type, ShapeType::Group);
        assert_eq!(inner.name.as_deref(), Some("Inner"));
        assert_eq!(inner.left, Some(5));
        let leaf = &inner.shapes.as_ref().unwrap()[0];
        assert_eq!(leaf.name.as_deref(), Some("Leaf"));
    }

    #[test]
    fn sibling_group_flat_shapes_order_is_preserved() {
        let xml = br#"
        <p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
               xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
          <p:cSld><p:spTree>
            <p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>
            <p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>
            <p:sp><p:nvSpPr><p:cNvPr id="2" name="A"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="10" cy="10"/></a:xfrm></p:spPr></p:sp>
            <p:grpSp>
              <p:nvGrpSpPr><p:cNvPr id="3" name="G"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>
              <p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="10" cy="10"/><a:chOff x="0" y="0"/><a:chExt cx="10" cy="10"/></a:xfrm></p:grpSpPr>
              <p:sp><p:nvSpPr><p:cNvPr id="4" name="B"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="10" cy="10"/></a:xfrm></p:spPr></p:sp>
            </p:grpSp>
            <p:sp><p:nvSpPr><p:cNvPr id="5" name="C"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="10" cy="10"/></a:xfrm></p:spPr></p:sp>
          </p:spTree></p:cSld>
        </p:sld>
        "#;
        let shapes = parse_slide_shapes(xml, &empty_map()).unwrap();
        assert_eq!(shapes.len(), 3);
        assert_eq!(shapes[0].name.as_deref(), Some("A"));
        assert_eq!(shapes[1].shape_type, ShapeType::Group);
        assert_eq!(shapes[1].name.as_deref(), Some("G"));
        assert_eq!(
            shapes[1].shapes.as_ref().unwrap()[0].name.as_deref(),
            Some("B")
        );
        assert_eq!(shapes[2].name.as_deref(), Some("C"));
    }
}
