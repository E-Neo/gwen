//! XML emitters.  This is a deliberately literal port of pptxgenjs's
//! `genXml*` / `makeXml*` functions: formatting, ordering and even the
//! whitespace in the output are part of gwen's compatibility contract.

use serde_json::{Value, json};

use super::*;

const DEF_TEXT_SHADOW_COLOR: &str = "000000";
const BULLET_DEFAULT: &str = "&#x2022;";

fn placeholder_type(name: &str) -> Option<&'static str> {
    match name {
        "title" => Some("title"),
        "body" => Some("body"),
        "image" => Some("pic"),
        "chart" => Some("chart"),
        "table" => Some("tbl"),
        "media" => Some("media"),
        _ => None,
    }
}

fn is_placeholder_type(name: &str) -> bool {
    matches!(name, "title" | "body" | "pic" | "chart" | "tbl" | "media")
}

fn is_hex_color(s: &str) -> bool {
    s.len() == 6 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_scheme_color(s: &str) -> bool {
    matches!(
        s,
        "tx1"
            | "tx2"
            | "bg1"
            | "bg2"
            | "accent1"
            | "accent2"
            | "accent3"
            | "accent4"
            | "accent5"
            | "accent6"
    )
}

pub(crate) fn create_color_element(color: &str, inner: &str) -> String {
    let mut color = color.replace('#', "");
    if !is_hex_color(&color) && !is_scheme_color(&color) {
        color = DEF_FONT_COLOR.into();
    }
    let tag = if is_hex_color(&color) {
        "srgbClr"
    } else {
        "schemeClr"
    };
    let attr = if is_hex_color(&color) {
        color.to_ascii_uppercase()
    } else {
        color
    };
    if inner.is_empty() {
        format!("<a:{tag} val=\"{attr}\"/>")
    } else {
        format!("<a:{tag} val=\"{attr}\">{inner}</a:{tag}>")
    }
}

pub(crate) fn gen_xml_color_selection(props: &Value) -> String {
    let mut fill_type = "solid";
    let mut color = String::new();
    let mut inner = String::new();
    match props {
        Value::String(s) => color = s.clone(),
        Value::Object(m) => {
            if let Some(t) = m.get("type").and_then(Value::as_str) {
                fill_type = t;
            }
            if let Some(c) = m.get("color") {
                color = js_to_string(c);
            }
            if let Some(a) = m.get("alpha").filter(|v| truthy(Some(v))) {
                inner.push_str(&format!(
                    "<a:alpha val=\"{}\"/>",
                    js_round((100.0 - js_number(a)) * 1000.0)
                ));
            }
            if let Some(a) = m.get("transparency").filter(|v| truthy(Some(v))) {
                inner.push_str(&format!(
                    "<a:alpha val=\"{}\"/>",
                    js_round((100.0 - js_number(a)) * 1000.0)
                ));
            }
        }
        _ => {}
    }
    if fill_type == "solid" {
        format!(
            "<a:solidFill>{}</a:solidFill>",
            create_color_element(&color, &inner)
        )
    } else {
        String::new()
    }
}

fn gen_xml_run_properties(opts: &Opts, is_default: bool) -> String {
    let mut out = String::new();
    let tag = if is_default { "a:defRPr" } else { "a:rPr" };
    let lang = opts
        .get("lang")
        .filter(|v| truthy(Some(v)))
        .map(js_to_string);
    out.push('<');
    out.push_str(tag);
    out.push_str(" lang=\"");
    out.push_str(lang.as_deref().unwrap_or("en-US"));
    out.push('"');
    if lang.is_some() {
        out.push_str(" altLang=\"en-US\"");
    }
    if let Some(n) = opt_num(opts, "fontSize") {
        out.push_str(&format!(" sz=\"{}\"", js_round(n * 100.0)));
    }
    if truthy(opts.get("bold")) {
        out.push_str(&format!(
            " b=\"{}\"",
            if opt_bool(opts, "bold") { "1" } else { "0" }
        ));
    }
    if truthy(opts.get("italic")) {
        out.push_str(&format!(
            " i=\"{}\"",
            if opt_bool(opts, "italic") { "1" } else { "0" }
        ));
    }
    if truthy(opts.get("strike")) {
        let strike = match opts.get("strike") {
            Some(Value::String(s)) => s.clone(),
            _ => "sngStrike".into(),
        };
        out.push_str(&format!(" strike=\"{strike}\""));
    }
    let underline = opts.get("underline");
    let mut underline_style: Option<String> = None;
    if let Some(u) = underline.and_then(Value::as_object)
        && let Some(style) = u.get("style").filter(|v| truthy(Some(v)))
    {
        underline_style = Some(js_to_string(style));
    } else if let Some(Value::String(u)) = underline {
        underline_style = Some(u.clone());
    } else if truthy(opts.get("hyperlink")) {
        underline_style = Some("sng".into());
    }
    if let Some(u) = underline_style {
        out.push_str(&format!(" u=\"{u}\""));
    }
    if let Some(b) = opt_num(opts, "baseline") {
        out.push_str(&format!(" baseline=\"{}\"", js_round(b * 50.0)));
    } else if truthy(opts.get("subscript")) {
        out.push_str(" baseline=\"-40000\"");
    } else if truthy(opts.get("superscript")) {
        out.push_str(" baseline=\"30000\"");
    }
    if let Some(c) = opt_num(opts, "charSpacing") {
        out.push_str(&format!(" spc=\"{}\" kern=\"0\"", js_round(c * 100.0)));
    }
    out.push_str(" dirty=\"0\">");

    let has_color = truthy(opts.get("color"));
    let font_face = opt_string(opts, "fontFace");
    let outline = opts.get("outline").and_then(Value::as_object);
    let underline_color = underline
        .and_then(Value::as_object)
        .and_then(|u| u.get("color"))
        .filter(|v| truthy(Some(v)));
    if has_color || font_face.is_some() || outline.is_some() || underline_color.is_some() {
        if let Some(o) = outline {
            let size = o
                .get("size")
                .filter(|v| truthy(Some(v)))
                .map(js_number)
                .unwrap_or(0.75);
            let color = o
                .get("color")
                .filter(|v| truthy(Some(v)))
                .cloned()
                .unwrap_or_else(|| json!("FFFFFF"));
            out.push_str(&format!(
                "<a:ln w=\"{}\">{}</a:ln>",
                val_to_pts(&json!(size)),
                gen_xml_color_selection(&color)
            ));
        }
        if has_color {
            let color = opts.get("color").cloned().unwrap_or(Value::Null);
            let transparency = opts.get("transparency").cloned().unwrap_or(json!(0));
            out.push_str(&gen_xml_color_selection(&json!({
                "color": color,
                "transparency": transparency
            })));
        }
        if let Some(h) = opts.get("highlight").filter(|v| truthy(Some(v))) {
            out.push_str(&format!(
                "<a:highlight>{}</a:highlight>",
                create_color_element(&js_to_string(h), "")
            ));
        }
        if let Some(c) = underline_color {
            out.push_str(&format!(
                "<a:uFill>{}</a:uFill>",
                gen_xml_color_selection(c)
            ));
        }
        if let Some(glow) = opts.get("glow").and_then(Value::as_object) {
            let size = glow
                .get("size")
                .filter(|v| truthy(Some(v)))
                .map(js_number)
                .unwrap_or(8.0);
            let color = glow
                .get("color")
                .filter(|v| truthy(Some(v)))
                .map(js_to_string)
                .unwrap_or_else(|| "FFFFFF".into());
            let opacity = glow
                .get("opacity")
                .filter(|v| truthy(Some(v)))
                .map(js_number)
                .unwrap_or(0.75);
            out.push_str(&format!(
                "<a:effectLst><a:glow rad=\"{}\">{}</a:glow></a:effectLst>",
                js_round(size * ONEPT),
                create_color_element(
                    &color,
                    &format!("<a:alpha val=\"{}\"/>", js_round(opacity * 100_000.0))
                )
            ));
        }
        if let Some(face) = font_face {
            out.push_str(&format!(
                "<a:latin typeface=\"{face}\" pitchFamily=\"34\" charset=\"0\"/><a:ea typeface=\"{face}\" pitchFamily=\"34\" charset=\"-122\"/><a:cs typeface=\"{face}\" pitchFamily=\"34\" charset=\"-120\"/>"
            ));
        }
    }
    if let Some(h) = opts.get("hyperlink").and_then(Value::as_object) {
        let r_id = h.get("_rId").map(js_to_string).unwrap_or_default();
        let tooltip = h
            .get("tooltip")
            .filter(|v| truthy(Some(v)))
            .map(encode_xml)
            .unwrap_or_default();
        let has_url = h.get("url").filter(|v| truthy(Some(v))).is_some();
        let has_slide = h.get("slide").filter(|v| truthy(Some(v))).is_some();
        if has_url {
            out.push_str(&format!(
                "<a:hlinkClick r:id=\"rId{r_id}\" invalidUrl=\"\" action=\"\" tgtFrame=\"\" tooltip=\"{tooltip}\" history=\"1\" highlightClick=\"0\" endSnd=\"0\"{}",
                if has_color { ">" } else { "/>" }
            ));
        } else if has_slide {
            out.push_str(&format!(
                "<a:hlinkClick r:id=\"rId{r_id}\" action=\"ppaction://hlinksldjump\" tooltip=\"{tooltip}\"{}",
                if has_color { ">" } else { "/>" }
            ));
        }
        if has_color {
            out.push_str(" <a:extLst>");
            out.push_str("  <a:ext uri=\"{A12FA001-AC4F-418D-AE19-62706E023703}\">");
            out.push_str("   <ahyp:hlinkClr xmlns:ahyp=\"http://schemas.microsoft.com/office/drawing/2018/hyperlinkcolor\" val=\"tx\"/>");
            out.push_str("  </a:ext>");
            out.push_str(" </a:extLst>");
            out.push_str("</a:hlinkClick>");
        }
    }
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
    out
}

pub(crate) fn gen_xml_paragraph_properties(opts: &Opts, is_default: bool) -> String {
    let tag = if is_default { "a:lvl1pPr" } else { "a:pPr" };
    let mut bullet_mar_l = val_to_pts(&json!(DEF_BULLET_MARGIN));
    let mut out = format!(
        "<{tag}{}",
        if opt_bool(opts, "rtlMode") {
            " rtl=\"1\" "
        } else {
            ""
        }
    );
    let mut ln_spc = String::new();
    let mut para_spc = String::new();
    let mut bullet_xml = String::new();
    let mut tab_stops_xml = String::new();

    if let Some(align) = opts
        .get("align")
        .filter(|v| truthy(Some(v)))
        .map(js_to_string)
    {
        out.push_str(match align.as_str() {
            "left" => " algn=\"l\"",
            "right" => " algn=\"r\"",
            "center" => " algn=\"ctr\"",
            "justify" => " algn=\"just\"",
            _ => "",
        });
    }
    if let Some(n) = opt_num(opts, "lineSpacing") {
        ln_spc = format!(
            "<a:lnSpc><a:spcPts val=\"{}\"/></a:lnSpc>",
            js_round(n * 100.0)
        );
    } else if let Some(n) = opt_num(opts, "lineSpacingMultiple") {
        ln_spc = format!(
            "<a:lnSpc><a:spcPct val=\"{}\"/></a:lnSpc>",
            js_round(n * 100_000.0)
        );
    }
    if let Some(l) = opts.get("indentLevel").filter(|v| truthy(Some(v))) {
        let n = js_number(l);
        if !n.is_nan() && n > 0.0 {
            out.push_str(&format!(" lvl=\"{}\"", js_to_string(l)));
        }
    }
    if let Some(v) = opts.get("paraSpaceBefore").filter(|v| truthy(Some(v))) {
        let n = js_number(v);
        if !n.is_nan() && n > 0.0 {
            para_spc.push_str(&format!(
                "<a:spcBef><a:spcPts val=\"{}\"/></a:spcBef>",
                js_round(n * 100.0)
            ));
        }
    }
    if let Some(v) = opts.get("paraSpaceAfter").filter(|v| truthy(Some(v))) {
        let n = js_number(v);
        if !n.is_nan() && n > 0.0 {
            para_spc.push_str(&format!(
                "<a:spcAft><a:spcPts val=\"{}\"/></a:spcAft>",
                js_round(n * 100.0)
            ));
        }
    }
    let bullet = opts.get("bullet");
    let indent = opt_num(opts, "indentLevel").filter(|n| *n > 0.0);
    if let Some(Value::Object(b)) = bullet {
        if let Some(i) = b.get("indent").filter(|v| truthy(Some(v))) {
            bullet_mar_l = val_to_pts(i);
        }
        let effective = if let Some(level) = indent {
            bullet_mar_l + bullet_mar_l * level
        } else {
            bullet_mar_l
        };
        if let Some(t) = b.get("type").filter(|v| truthy(Some(v))) {
            if js_to_string(t).eq_ignore_ascii_case("number") {
                out.push_str(&format!(
                    " marL=\"{}\" indent=\"-{}\"",
                    js_round(effective),
                    js_round(bullet_mar_l)
                ));
                let style = b
                    .get("style")
                    .filter(|v| truthy(Some(v)))
                    .map(js_to_string)
                    .unwrap_or_else(|| "arabicPeriod".into());
                let start = b
                    .get("numberStartAt")
                    .or_else(|| b.get("startAt"))
                    .filter(|v| truthy(Some(v)))
                    .map(js_to_string)
                    .unwrap_or_else(|| "1".into());
                bullet_xml = format!(
                    "<a:buSzPct val=\"100000\"/><a:buFont typeface=\"+mj-lt\"/><a:buAutoNum type=\"{style}\" startAt=\"{start}\"/>"
                );
            }
        } else if let Some(code) = b
            .get("characterCode")
            .or_else(|| b.get("code"))
            .filter(|v| truthy(Some(v)))
            .map(js_to_string)
        {
            let valid = code.len() == 4 && code.bytes().all(|c| c.is_ascii_hexdigit());
            let code = if valid {
                format!("&#x{code};")
            } else {
                BULLET_DEFAULT.into()
            };
            out.push_str(&format!(
                " marL=\"{}\" indent=\"-{}\"",
                js_round(effective),
                js_round(bullet_mar_l)
            ));
            bullet_xml = format!("<a:buSzPct val=\"100000\"/><a:buChar char=\"{code}\"/>");
        } else {
            out.push_str(&format!(
                " marL=\"{}\" indent=\"-{}\"",
                js_round(effective),
                js_round(bullet_mar_l)
            ));
            bullet_xml =
                format!("<a:buSzPct val=\"100000\"/><a:buChar char=\"{BULLET_DEFAULT}\"/>");
        }
    } else if truthy(bullet) {
        let effective = if let Some(level) = indent {
            bullet_mar_l + bullet_mar_l * level
        } else {
            bullet_mar_l
        };
        out.push_str(&format!(
            " marL=\"{}\" indent=\"-{}\"",
            js_round(effective),
            js_round(bullet_mar_l)
        ));
        bullet_xml = format!("<a:buSzPct val=\"100000\"/><a:buChar char=\"{BULLET_DEFAULT}\"/>");
    } else {
        out.push_str(" indent=\"0\" marL=\"0\"");
        bullet_xml = "<a:buNone/>".into();
    }
    if let Some(Value::Array(stops)) = opts.get("tabStops") {
        let mut tabs = String::new();
        for stop in stops {
            let Some(stop) = stop.as_object() else {
                continue;
            };
            let pos = stop
                .get("position")
                .filter(|v| truthy(Some(v)))
                .cloned()
                .unwrap_or_else(|| json!(1));
            let align = stop
                .get("alignment")
                .filter(|v| truthy(Some(v)))
                .map(js_to_string)
                .unwrap_or_else(|| "l".into());
            tabs.push_str(&format!(
                "<a:tab pos=\"{}\" algn=\"{align}\"/>",
                inch2emu(&pos)
            ));
        }
        tab_stops_xml = format!("<a:tabLst>{tabs}</a:tabLst>");
    }
    out.push('>');
    out.push_str(&ln_spc);
    out.push_str(&para_spc);
    out.push_str(&bullet_xml);
    out.push_str(&tab_stops_xml);
    if is_default {
        out.push_str(&gen_xml_run_properties(opts, true));
    }
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
    out
}

fn gen_xml_text_run(run: &Run) -> String {
    if run.text.is_empty() {
        String::new()
    } else {
        format!(
            "<a:r>{}<a:t>{}</a:t></a:r>",
            gen_xml_run_properties(&run.options, false),
            encode_xml_str(&run.text)
        )
    }
}

fn gen_xml_body_properties(obj: &Obj) -> String {
    if obj.kind == ObjKind::Text
        && let Some(body) = opt_obj(&obj.options, "_bodyProp")
    {
        let mut out = String::from("<a:bodyPr");
        out.push_str(if truthy(body.get("wrap")) {
            " wrap=\"square\""
        } else {
            " wrap=\"none\""
        });
        for (key, attr) in [
            ("lIns", "lIns"),
            ("tIns", "tIns"),
            ("rIns", "rIns"),
            ("bIns", "bIns"),
        ] {
            if let Some(v) = body.get(key)
                && (!js_number(v).is_nan())
            {
                out.push_str(&format!(" {attr}=\"{}\"", js_to_string(v)));
            }
        }
        out.push_str(" rtlCol=\"0\"");
        if let Some(a) = body.get("anchor").filter(|v| truthy(Some(v))) {
            out.push_str(&format!(" anchor=\"{}\"", js_to_string(a)));
        }
        if let Some(v) = body.get("vert").filter(|v| truthy(Some(v))) {
            out.push_str(&format!(" vert=\"{}\"", js_to_string(v)));
        }
        out.push('>');
        if let Some(fit) = obj.options.get("fit").and_then(Value::as_str) {
            match fit {
                "none" => {}
                "shrink" => out.push_str("<a:normAutofit/>"),
                "resize" => out.push_str("<a:spAutoFit/>"),
                _ => {}
            }
        }
        if opt_bool(&obj.options, "shrinkText") {
            out.push_str("<a:normAutofit/>");
        }
        if truthy(body.get("autoFit")) {
            out.push_str("<a:spAutoFit/>");
        }
        out.push_str("</a:bodyPr>");
        out
    } else {
        "<a:bodyPr wrap=\"square\" rtlCol=\"0\"></a:bodyPr>".into()
    }
}

fn gen_xml_text_body(obj: &mut Obj) -> String {
    let mut shape_opts = obj.options.clone();
    let Some(runs) = obj.text.clone() else {
        return String::new();
    };
    let mut out = String::from("<p:txBody>");
    out.push_str(&gen_xml_body_properties(obj));
    if shape_opts
        .get("h")
        .is_some_and(|v| !v.is_null() && js_number(v) == 0.0)
        && truthy(shape_opts.get("line"))
        && truthy(shape_opts.get("align"))
    {
        out.push_str("<a:lstStyle><a:lvl1pPr algn=\"l\"/></a:lstStyle>");
    } else if obj.kind == ObjKind::Placeholder {
        out.push_str(&format!(
            "<a:lstStyle>{}</a:lstStyle>",
            gen_xml_paragraph_properties(&shape_opts, true)
        ));
    } else {
        out.push_str("<a:lstStyle/>");
    }

    // STEP 3/4: normalise runs and split embedded line breaks.
    let mut arr: Vec<Run> = Vec::new();
    for (idx, mut run) in runs.into_iter().enumerate() {
        let text = run
            .text
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace('\n', "\r\n");
        if idx == 0
            && !truthy(run.options.get("bullet"))
            && let Some(b) = shape_opts.get("bullet").filter(|v| truthy(Some(v)))
        {
            run.options.insert("bullet".into(), b.clone());
        }
        if text.contains("\r\n") && !text.ends_with('\n') {
            for line in text.split("\r\n") {
                run.options.insert("breakLine".into(), Value::Bool(true));
                arr.push(Run {
                    text: line.to_string(),
                    options: run.options.clone(),
                });
            }
        } else {
            run.text = text;
            arr.push(run);
        }
    }

    // STEP 5: group runs into paragraphs.
    let mut lines: Vec<Vec<Run>> = Vec::new();
    let mut current: Vec<Run> = Vec::new();
    for idx in 0..arr.len() {
        let has_align = truthy(arr[idx].options.get("align")) || truthy(shape_opts.get("align"));
        if !current.is_empty() && has_align {
            let prev_align = arr[idx - 1].options.get("align");
            if arr[idx].options.get("align") != prev_align {
                lines.push(std::mem::take(&mut current));
            }
        } else if !current.is_empty() && truthy(arr[idx].options.get("bullet")) {
            lines.push(std::mem::take(&mut current));
            arr[idx]
                .options
                .insert("breakLine".into(), Value::Bool(false));
        }
        current.push(arr[idx].clone());
        if !current.is_empty() && truthy(arr[idx].options.get("breakLine")) && idx + 1 < arr.len() {
            lines.push(std::mem::take(&mut current));
        }
        if idx + 1 == arr.len() {
            lines.push(std::mem::take(&mut current));
        }
    }

    // STEP 6: emit paragraphs.
    for line in lines {
        let mut reqs_closing_font_size = false;
        out.push_str("<a:p>");
        for (idx, mut text_obj) in line.into_iter().enumerate() {
            if idx > 0 && truthy(text_obj.options.get("softBreakBefore")) {
                out.push_str("<a:br/>");
            }
            for key in [
                "align",
                "lineSpacing",
                "lineSpacingMultiple",
                "indentLevel",
                "paraSpaceBefore",
                "paraSpaceAfter",
            ] {
                if !truthy(text_obj.options.get(key))
                    && let Some(v) = shape_opts.get(key)
                {
                    text_obj.options.insert(key.into(), v.clone());
                }
            }
            let ppr = gen_xml_paragraph_properties(&text_obj.options, false);
            out.push_str(&ppr.replace("<a:pPr></a:pPr>", ""));
            let inherited = shape_opts.clone();
            for (key, val) in inherited {
                if key == "bullet" {
                    continue;
                }
                if !truthy(text_obj.options.get(&key)) {
                    text_obj.options.insert(key, val);
                }
            }
            out.push_str(&gen_xml_text_run(&text_obj));
            if (text_obj.text.is_empty() && truthy(shape_opts.get("fontSize")))
                || truthy(text_obj.options.get("fontSize"))
            {
                reqs_closing_font_size = true;
                if !truthy(shape_opts.get("fontSize"))
                    && let Some(v) = text_obj.options.get("fontSize")
                {
                    shape_opts.insert("fontSize".into(), v.clone());
                }
            }
        }
        let lang = shape_opts
            .get("lang")
            .filter(|v| truthy(Some(v)))
            .map(js_to_string)
            .unwrap_or_else(|| "en-US".into());
        if reqs_closing_font_size {
            out.push_str(&format!(
                "<a:endParaRPr lang=\"{lang}\"{} dirty=\"0\"/>",
                opt_num(&shape_opts, "fontSize")
                    .map(|n| format!(" sz=\"{}\"", js_round(n * 100.0)))
                    .unwrap_or_default()
            ));
        } else {
            out.push_str(&format!("<a:endParaRPr lang=\"{lang}\" dirty=\"0\"/>"));
        }
        out.push_str("</a:p>");
    }
    if !out.contains("<a:p>") {
        out.push_str("<a:p><a:endParaRPr/></a:p>");
    }
    out.push_str("</p:txBody>");
    out
}

fn gen_xml_placeholder(options: &Opts, has_custom_prompt: bool) -> String {
    let idx = options
        .get("_placeholderIdx")
        .filter(|v| truthy(Some(v)))
        .map(js_to_string)
        .unwrap_or_default();
    let typ = options
        .get("_placeholderType")
        .filter(|v| truthy(Some(v)))
        .map(js_to_string)
        .unwrap_or_default();
    let type_ = placeholder_type(&typ).map(String::from).unwrap_or_default();
    format!(
        "<p:ph\n\t\t{}\n\t\t{}\n\t\t{}\n\t\t/>",
        if idx.is_empty() {
            String::new()
        } else {
            format!(" idx=\"{idx}\"")
        },
        if !type_.is_empty() && is_placeholder_type(&type_) {
            format!(" type=\"{type_}\"")
        } else {
            String::new()
        },
        if has_custom_prompt {
            " hasCustomPrompt=\"1\"".to_string()
        } else {
            String::new()
        }
    )
}

fn image_sizing_xml(kind: &str, img: (f64, f64), bx: (f64, f64, f64, f64)) -> String {
    let (iw, ih) = img;
    let (bw, bh, bx_, by) = bx;
    let img_ratio = ih / iw;
    let box_ratio = bh / bw;
    match kind {
        "cover" => {
            let box_based = box_ratio > img_ratio;
            let width = if box_based { bh / img_ratio } else { bw };
            let height = if box_based { bh } else { bw * img_ratio };
            let hz = js_round(1e5 * 0.5 * (1.0 - bw / width));
            let vz = js_round(1e5 * 0.5 * (1.0 - bh / height));
            format!("<a:srcRect l=\"{hz}\" r=\"{hz}\" t=\"{vz}\" b=\"{vz}\"/><a:stretch/>")
        }
        "contain" => {
            let width_based = box_ratio > img_ratio;
            let width = if width_based { bw } else { bh / img_ratio };
            let height = if width_based { bw * img_ratio } else { bh };
            let hz = js_round(1e5 * 0.5 * (1.0 - bw / width));
            let vz = js_round(1e5 * 0.5 * (1.0 - bh / height));
            format!("<a:srcRect l=\"{hz}\" r=\"{hz}\" t=\"{vz}\" b=\"{vz}\"/><a:stretch/>")
        }
        "crop" => {
            let l = bx_;
            let r = iw - (bx_ + bw);
            let t = by;
            let b = ih - (by + bh);
            let lp = js_round(1e5 * (l / iw));
            let rp = js_round(1e5 * (r / iw));
            let tp = js_round(1e5 * (t / ih));
            let bp = js_round(1e5 * (b / ih));
            format!("<a:srcRect l=\"{lp}\" r=\"{rp}\" t=\"{tp}\" b=\"{bp}\"/><a:stretch/>")
        }
        _ => String::new(),
    }
}

fn shadow_parts(opts: &Opts) -> Option<(String, String, f64, f64, f64, f64, String)> {
    let shadow = opts.get("shadow").and_then(Value::as_object)?;
    let ty = shadow
        .get("type")
        .filter(|v| truthy(Some(v)))
        .map(js_to_string)
        .unwrap_or_else(|| "outer".into());
    if ty == "none" {
        return None;
    }
    let blur = shadow
        .get("blur")
        .filter(|v| truthy(Some(v)))
        .map(val_to_pts)
        .unwrap_or_else(|| val_to_pts(&json!(8)));
    let offset = shadow
        .get("offset")
        .filter(|v| truthy(Some(v)))
        .map(val_to_pts)
        .unwrap_or_else(|| val_to_pts(&json!(4)));
    let angle = js_round(
        shadow
            .get("angle")
            .filter(|v| truthy(Some(v)))
            .map(js_number)
            .unwrap_or(270.0)
            * 60_000.0,
    );
    let opacity = js_round(
        shadow
            .get("opacity")
            .filter(|v| truthy(Some(v)))
            .map(js_number)
            .unwrap_or(0.75)
            * 100_000.0,
    );
    let color = shadow
        .get("color")
        .filter(|v| truthy(Some(v)))
        .map(js_to_string)
        .unwrap_or_else(|| DEF_TEXT_SHADOW_COLOR.into());
    let attrs = if ty == "outer" {
        "sx=\"100000\" sy=\"100000\" kx=\"0\" ky=\"0\" algn=\"bl\" rotWithShape=\"0\""
    } else {
        ""
    };
    Some((ty, attrs.into(), blur, offset, angle, opacity, color))
}

/// Shape shadows: pptxgenjs's text/shape branch has extra spaces and (for
/// inner shadows) closes the wrong element.  Both quirks are preserved.
fn shape_shadow_xml(opts: &Opts) -> String {
    let Some((ty, attrs, blur, offset, angle, opacity, color)) = shadow_parts(opts) else {
        return String::new();
    };
    format!(
        "<a:effectLst> <a:{ty}Shdw {attrs} blurRad=\"{blur}\" dist=\"{offset}\" dir=\"{angle}\"> <a:srgbClr val=\"{color}\"> <a:alpha val=\"{opacity}\"/></a:srgbClr> </a:outerShdw></a:effectLst>"
    )
}

/// Image shadows use the compact form and balance the element name.
fn image_shadow_xml(opts: &Opts) -> String {
    let Some((ty, attrs, blur, offset, angle, opacity, color)) = shadow_parts(opts) else {
        return String::new();
    };
    format!(
        "<a:effectLst><a:{ty}Shdw {attrs} blurRad=\"{blur}\" dist=\"{offset}\" dir=\"{angle}\"><a:srgbClr val=\"{color}\"><a:alpha val=\"{opacity}\"/></a:srgbClr></a:{ty}Shdw></a:effectLst>"
    )
}

fn line_xml(opts: &Opts) -> String {
    let Some(line) = opts.get("line").and_then(Value::as_object) else {
        return String::new();
    };
    let mut out = if let Some(w) = line.get("width").filter(|v| truthy(Some(v))) {
        format!("<a:ln w=\"{}\">", val_to_pts(w))
    } else {
        "<a:ln>".into()
    };
    if let Some(color) = line.get("color").filter(|v| truthy(Some(v))) {
        out.push_str(&gen_xml_color_selection(color));
    }
    if let Some(dash) = line.get("dashType").filter(|v| truthy(Some(v))) {
        out.push_str(&format!("<a:prstDash val=\"{}\"/>", js_to_string(dash)));
    }
    if let Some(v) = line.get("beginArrowType").filter(|v| truthy(Some(v))) {
        out.push_str(&format!("<a:headEnd type=\"{}\"/>", js_to_string(v)));
    }
    if let Some(v) = line.get("endArrowType").filter(|v| truthy(Some(v))) {
        out.push_str(&format!("<a:tailEnd type=\"{}\"/>", js_to_string(v)));
    }
    out.push_str("</a:ln>");
    out
}

fn slide_number_xml(target: &Target, deck: &Deck) -> String {
    let Some(props) = target.slide_number.as_ref() else {
        return String::new();
    };
    let mut out = String::new();
    out.push_str("<p:sp>");
    out.push_str(" <p:nvSpPr>");
    out.push_str("  <p:cNvPr id=\"25\" name=\"Slide Number Placeholder 0\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr>");
    out.push_str("  <p:nvPr><p:ph type=\"sldNum\" sz=\"quarter\" idx=\"4294967295\"/></p:nvPr>");
    out.push_str(" </p:nvSpPr>");
    out.push_str(" <p:spPr>");
    let x = props.get("x").cloned().unwrap_or(json!(0));
    let y = props.get("y").cloned().unwrap_or(json!(0));
    let cx = props
        .get("w")
        .filter(|v| truthy(Some(v)))
        .map(|v| smart_number(v, 'X', deck.layout_size()).to_string())
        .unwrap_or_else(|| "800000".into());
    let cy = props
        .get("h")
        .filter(|v| truthy(Some(v)))
        .map(|v| smart_number(v, 'Y', deck.layout_size()).to_string())
        .unwrap_or_else(|| "300000".into());
    out.push_str(&format!(
        "<a:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm>",
        smart_number(&x, 'X', deck.layout_size()),
        smart_number(&y, 'Y', deck.layout_size())
    ));
    out.push_str(" <a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom>");
    out.push_str(" <a:extLst><a:ext uri=\"{C572A759-6A51-4108-AA02-DFA0A04FC94B}\"><ma14:wrappingTextBoxFlag val=\"0\" xmlns:ma14=\"http://schemas.microsoft.com/office/mac/drawingml/2011/main\"/></a:ext></a:extLst>");
    out.push_str("</p:spPr>");
    out.push_str("<p:txBody>");
    out.push_str("<a:bodyPr");
    if let Some(Value::Array(m)) = props.get("margin") {
        let at = |i: usize| m.get(i).cloned().unwrap_or(json!(0));
        out.push_str(&format!(" lIns=\"{}\"", val_to_pts(&at(3))));
        out.push_str(&format!(" tIns=\"{}\"", val_to_pts(&at(0))));
        out.push_str(&format!(" rIns=\"{}\"", val_to_pts(&at(1))));
        out.push_str(&format!(" bIns=\"{}\"", val_to_pts(&at(2))));
    } else if let Some(m) = props.get("margin").filter(|v| v.is_number()) {
        let m = val_to_pts(m);
        out.push_str(&format!(
            " lIns=\"{m}\" tIns=\"{m}\" rIns=\"{m}\" bIns=\"{m}\""
        ));
    }
    if let Some(v) = props.get("valign").filter(|v| truthy(Some(v))) {
        let v = js_to_string(v)
            .replace("top", "t")
            .replace("middle", "ctr")
            .replace("bottom", "b");
        out.push_str(&format!(" anchor=\"{v}\""));
    }
    out.push_str("/>");
    out.push_str("  <a:lstStyle><a:lvl1pPr>");
    if props.get("fontFace").is_some_and(|v| truthy(Some(v)))
        || props.get("fontSize").is_some_and(|v| truthy(Some(v)))
        || props.get("color").is_some_and(|v| truthy(Some(v)))
    {
        let size = props
            .get("fontSize")
            .filter(|v| truthy(Some(v)))
            .map(js_number)
            .unwrap_or(12.0);
        out.push_str(&format!("<a:defRPr sz=\"{}\">", js_round(size * 100.0)));
        if let Some(color) = props.get("color").filter(|v| truthy(Some(v))) {
            out.push_str(&gen_xml_color_selection(color));
        }
        if let Some(face) = props.get("fontFace").filter(|v| truthy(Some(v))) {
            let face = js_to_string(face);
            out.push_str(&format!(
                "<a:latin typeface=\"{face}\"/><a:ea typeface=\"{face}\"/><a:cs typeface=\"{face}\"/>"
            ));
        }
        out.push_str("</a:defRPr>");
    }
    out.push_str("</a:lvl1pPr></a:lstStyle>");
    out.push_str("<a:p>");
    let align = props
        .get("align")
        .filter(|v| truthy(Some(v)))
        .map(js_to_string)
        .unwrap_or_else(|| "left".into());
    if align.starts_with('l') {
        out.push_str("<a:pPr algn=\"l\"/>");
    } else if align.starts_with('c') {
        out.push_str("<a:pPr algn=\"ctr\"/>");
    } else if align.starts_with('r') {
        out.push_str("<a:pPr algn=\"r\"/>");
    } else {
        out.push_str("<a:pPr algn=\"l\"/>");
    }
    out.push_str(&format!(
        "<a:fld id=\"{SLDNUMFLDID}\" type=\"slidenum\"><a:rPr b=\"{}\" lang=\"en-US\"/>",
        if opt_bool(props, "bold") { 1 } else { 0 }
    ));
    out.push_str("<a:t>‹#›</a:t></a:fld><a:endParaRPr lang=\"en-US\"/></a:p>");
    out.push_str("</p:txBody></p:sp>");
    out
}

fn slide_object_xml(target: &Target, deck: &Deck, placeholders: &[PlaceholderInfo]) -> String {
    let mut out = if let Some(name) = &target.name {
        format!("<p:cSld name=\"{name}\">")
    } else {
        "<p:cSld>".into()
    };
    if let Some(bg) = &target.background
        && bg.get("color").is_some_and(|v| truthy(Some(v)))
    {
        out.push_str(&format!(
            "<p:bg><p:bgPr>{}</p:bgPr></p:bg>",
            gen_xml_color_selection(&Value::Object(bg.clone()))
        ));
    }
    out.push_str("<p:spTree>");
    out.push_str(
        "<p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>",
    );
    out.push_str("<p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/>");
    out.push_str("<a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>");

    for (idx, original) in target.objects.iter().enumerate() {
        let mut obj = original.clone();
        if obj.kind == ObjKind::Notes {
            continue;
        }
        let mut x = 0.0;
        let mut y = 0.0;
        let mut cx = smart_number(&json!("75%"), 'X', deck.layout_size());
        let mut cy = 0.0;
        let mut location = String::new();
        let placeholder = if target.slide_layout.is_some() {
            obj.options.get("placeholder").and_then(|name| {
                placeholders
                    .iter()
                    .find(|p| p.options.get("placeholder") == Some(name))
            })
        } else {
            None
        };
        if let Some(v) = obj.options.get("x").filter(|v| !v.is_null()) {
            x = smart_number(v, 'X', deck.layout_size());
        }
        if let Some(v) = obj.options.get("y").filter(|v| !v.is_null()) {
            y = smart_number(v, 'Y', deck.layout_size());
        }
        if let Some(v) = obj.options.get("w").filter(|v| !v.is_null()) {
            cx = smart_number(v, 'X', deck.layout_size());
        }
        if let Some(v) = obj.options.get("h").filter(|v| !v.is_null()) {
            cy = smart_number(v, 'Y', deck.layout_size());
        }
        let mut img_width = cx;
        let mut img_height = cy;
        if let Some(ph) = placeholder {
            for (key, axis, value) in [
                ("x", 'X', &mut x),
                ("y", 'Y', &mut y),
                ("w", 'X', &mut cx),
                ("h", 'Y', &mut cy),
            ] {
                if let Some(v) = ph.options.get(key)
                    && (truthy(Some(v)) || v == &json!(0))
                {
                    *value = smart_number(v, axis, deck.layout_size());
                }
            }
        }
        if opt_bool(&obj.options, "flipH") {
            location.push_str(" flipH=\"1\"");
        }
        if opt_bool(&obj.options, "flipV") {
            location.push_str(" flipV=\"1\"");
        }
        if let Some(v) = obj.options.get("rotate").filter(|v| truthy(Some(v))) {
            location.push_str(&format!(" rot=\"{}\"", convert_rotation_degrees(v)));
        }

        match obj.kind {
            ObjKind::Text | ObjKind::Placeholder => {
                if !truthy(obj.options.get("line")) && cy == 0.0 {
                    cy = EMU * 0.3;
                }
                let mut body = obj
                    .options
                    .get("_bodyProp")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                if let Some(Value::Array(m)) = obj.options.get("margin") {
                    let at = |i: usize| m.get(i).cloned().unwrap_or(json!(0));
                    body.insert("lIns".into(), json!(val_to_pts(&at(0))));
                    body.insert("rIns".into(), json!(val_to_pts(&at(1))));
                    body.insert("bIns".into(), json!(val_to_pts(&at(2))));
                    body.insert("tIns".into(), json!(val_to_pts(&at(3))));
                } else if let Some(m) = obj.options.get("margin").filter(|v| v.is_number()) {
                    let m = val_to_pts(m);
                    for k in ["lIns", "rIns", "bIns", "tIns"] {
                        body.insert(k.into(), json!(m));
                    }
                }
                obj.options.insert("_bodyProp".into(), Value::Object(body));

                out.push_str("<p:sp>");
                out.push_str(&format!(
                    "<p:nvSpPr><p:cNvPr id=\"{}\" name=\"{}\">",
                    idx + 2,
                    obj.options
                        .get("objectName")
                        .map(js_to_string)
                        .unwrap_or_default()
                ));
                if let Some(h) = obj.options.get("hyperlink").and_then(Value::as_object) {
                    let tooltip = h
                        .get("tooltip")
                        .filter(|v| truthy(Some(v)))
                        .map(encode_xml)
                        .unwrap_or_default();
                    if h.get("url").is_some_and(|v| truthy(Some(v))) {
                        out.push_str(&format!(
                            "<a:hlinkClick r:id=\"rId{}\" tooltip=\"{tooltip}\"/>",
                            h.get("_rId").map(js_to_string).unwrap_or_default()
                        ));
                    }
                    if h.get("slide").is_some_and(|v| truthy(Some(v))) {
                        out.push_str(&format!(
                            "<a:hlinkClick r:id=\"rId{}\" tooltip=\"{tooltip}\" action=\"ppaction://hlinksldjump\"/>",
                            h.get("_rId").map(js_to_string).unwrap_or_default()
                        ));
                    }
                }
                out.push_str("</p:cNvPr>");
                let is_placeholder = obj.kind == ObjKind::Placeholder || placeholder.is_some();
                if is_placeholder {
                    out.push_str("<p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr>");
                } else if opt_bool(&obj.options, "isTextBox") {
                    out.push_str("<p:cNvSpPr txBox=\"1\"/>");
                } else {
                    out.push_str("<p:cNvSpPr/>");
                }
                if obj.kind == ObjKind::Placeholder {
                    out.push_str(&format!(
                        "<p:nvPr>{}</p:nvPr>",
                        gen_xml_placeholder(&obj.options, obj.has_custom_prompt)
                    ));
                } else {
                    out.push_str(&format!(
                        "<p:nvPr>{}</p:nvPr>",
                        placeholder
                            .map(|p| gen_xml_placeholder(&p.options, p.has_custom_prompt))
                            .unwrap_or_default()
                    ));
                }
                out.push_str("</p:nvSpPr><p:spPr>");
                out.push_str(&format!(
                    "<a:xfrm{location}><a:off x=\"{x}\" y=\"{y}\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm>"
                ));
                out.push_str(&format!("<a:prstGeom prst=\"{}\"><a:avLst>", obj.shape));
                if let Some(radius) = opt_num(&obj.options, "rectRadius") {
                    out.push_str(&format!(
                        "<a:gd name=\"adj\" fmla=\"val {}\"/>",
                        js_round((radius * EMU * 100_000.0) / cx.min(cy))
                    ));
                } else if let Some(Value::Array(range)) = obj.options.get("angleRange") {
                    for i in 0..2 {
                        if let Some(angle) = range.get(i) {
                            out.push_str(&format!(
                                "<a:gd name=\"adj{}\" fmla=\"val {}\" />",
                                i + 1,
                                convert_rotation_degrees(angle)
                            ));
                        }
                    }
                    if let Some(v) = obj.options.get("arcThicknessRatio") {
                        out.push_str(&format!(
                            "<a:gd name=\"adj3\" fmla=\"val {}\" />",
                            js_round(js_number(v) * 50_000.0)
                        ));
                    }
                }
                out.push_str("</a:avLst></a:prstGeom>");
                if let Some(fill) = obj.options.get("fill").filter(|v| truthy(Some(v))) {
                    out.push_str(&gen_xml_color_selection(fill));
                } else {
                    out.push_str("<a:noFill/>");
                }
                out.push_str(&line_xml(&obj.options));
                out.push_str(&shape_shadow_xml(&obj.options));
                out.push_str("</p:spPr>");
                out.push_str(&gen_xml_text_body(&mut obj));
                out.push_str("</p:sp>");
            }
            ObjKind::Image => {
                let image_rid = obj.image_rid.unwrap_or(0);
                out.push_str("<p:pic>");
                out.push_str("  <p:nvPicPr>");
                let descr = obj
                    .options
                    .get("altText")
                    .filter(|v| truthy(Some(v)))
                    .cloned()
                    .or_else(|| obj.image.clone().map(Value::String))
                    .unwrap_or(Value::String(String::new()));
                out.push_str(&format!(
                    "<p:cNvPr id=\"{}\" name=\"{}\" descr=\"{}\">",
                    idx + 2,
                    obj.options
                        .get("objectName")
                        .map(js_to_string)
                        .unwrap_or_default(),
                    encode_xml(&descr)
                ));
                if let Some(h) = &obj.hyperlink {
                    let tooltip = h
                        .get("tooltip")
                        .filter(|v| truthy(Some(v)))
                        .map(encode_xml)
                        .unwrap_or_default();
                    let r_id = h.get("_rId").map(js_to_string).unwrap_or_default();
                    if h.get("url").is_some_and(|v| truthy(Some(v))) {
                        out.push_str(&format!(
                            "<a:hlinkClick r:id=\"rId{r_id}\" tooltip=\"{tooltip}\"/>"
                        ));
                    }
                    if h.get("slide").is_some_and(|v| truthy(Some(v))) {
                        out.push_str(&format!(
                            "<a:hlinkClick r:id=\"rId{r_id}\" tooltip=\"{tooltip}\" action=\"ppaction://hlinksldjump\"/>"
                        ));
                    }
                }
                out.push_str("    </p:cNvPr>");
                out.push_str("    <p:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></p:cNvPicPr>");
                out.push_str(&format!(
                    "    <p:nvPr>{}</p:nvPr>",
                    placeholder
                        .map(|p| gen_xml_placeholder(&p.options, p.has_custom_prompt))
                        .unwrap_or_default()
                ));
                out.push_str("  </p:nvPicPr>");
                out.push_str("<p:blipFill>");
                let is_svg = target
                    .media
                    .iter()
                    .find(|m| m.r_id == image_rid)
                    .map(|m| m.extn == "svg")
                    .unwrap_or(false);
                if is_svg {
                    out.push_str(&format!("<a:blip r:embed=\"rId{}\">", image_rid - 1));
                    if let Some(t) = obj.options.get("transparency").filter(|v| truthy(Some(v))) {
                        out.push_str(&format!(
                            " <a:alphaModFix amt=\"{}\"/>",
                            js_round((100.0 - js_number(t)) * 1000.0)
                        ));
                    }
                    out.push_str(" <a:extLst>");
                    out.push_str("  <a:ext uri=\"{96DAC541-7B7A-43D3-8B79-37D633B846F1}\">");
                    out.push_str(&format!(
                        "   <asvg:svgBlip xmlns:asvg=\"http://schemas.microsoft.com/office/drawing/2016/SVG/main\" r:embed=\"rId{image_rid}\"/>"
                    ));
                    out.push_str("  </a:ext>");
                    out.push_str(" </a:extLst>");
                    out.push_str("</a:blip>");
                } else {
                    out.push_str(&format!("<a:blip r:embed=\"rId{image_rid}\">"));
                    if let Some(t) = obj.options.get("transparency").filter(|v| truthy(Some(v))) {
                        out.push_str(&format!(
                            "<a:alphaModFix amt=\"{}\"/>",
                            js_round((100.0 - js_number(t)) * 1000.0)
                        ));
                    }
                    out.push_str("</a:blip>");
                }
                if let Some(sizing) = obj.options.get("sizing").and_then(Value::as_object) {
                    if let Some(kind) = sizing.get("type").and_then(Value::as_str) {
                        let bw = sizing
                            .get("w")
                            .filter(|v| truthy(Some(v)))
                            .map(|v| smart_number(v, 'X', deck.layout_size()))
                            .unwrap_or(cx);
                        let bh = sizing
                            .get("h")
                            .filter(|v| truthy(Some(v)))
                            .map(|v| smart_number(v, 'Y', deck.layout_size()))
                            .unwrap_or(cy);
                        let bx = smart_number(
                            sizing.get("x").unwrap_or(&json!(0)),
                            'X',
                            deck.layout_size(),
                        );
                        let by = smart_number(
                            sizing.get("y").unwrap_or(&json!(0)),
                            'Y',
                            deck.layout_size(),
                        );
                        out.push_str(&image_sizing_xml(
                            kind,
                            (img_width, img_height),
                            (bw, bh, bx, by),
                        ));
                        img_width = bw;
                        img_height = bh;
                    }
                } else {
                    out.push_str("  <a:stretch><a:fillRect/></a:stretch>");
                }
                out.push_str("</p:blipFill>");
                out.push_str("<p:spPr>");
                out.push_str(&format!(" <a:xfrm{location}>"));
                out.push_str(&format!("  <a:off x=\"{x}\" y=\"{y}\"/>"));
                out.push_str(&format!(
                    "  <a:ext cx=\"{img_width}\" cy=\"{img_height}\"/>"
                ));
                out.push_str(" </a:xfrm>");
                out.push_str(&format!(
                    " <a:prstGeom prst=\"{}\"><a:avLst/></a:prstGeom>",
                    if opt_bool(&obj.options, "rounding") {
                        "ellipse"
                    } else {
                        "rect"
                    }
                ));
                out.push_str(&image_shadow_xml(&obj.options));
                out.push_str("</p:spPr>");
                out.push_str("</p:pic>");
            }
            ObjKind::Notes => {}
        }
    }
    out.push_str(&slide_number_xml(target, deck));
    out.push_str("</p:spTree>");
    out.push_str("</p:cSld>");
    out
}

pub(crate) fn make_xml_slide(
    slide: &Target,
    deck: &Deck,
    placeholders: &[PlaceholderInfo],
) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>{CRLF}<p:sld xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\">{}</p:sld>",
        slide_object_xml(slide, deck, placeholders).replace(
            "</p:cSld>",
            "</p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>"
        )
    )
}

pub(crate) fn make_xml_layout(
    layout: &Target,
    deck: &Deck,
    placeholders: Option<&[PlaceholderInfo]>,
) -> String {
    let phs = placeholders.unwrap_or(&[]);
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\t\t<p:sldLayout xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" preserve=\"1\">\n\t\t{}\n\t\t<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>",
        slide_object_xml(layout, deck, phs)
    )
}

fn slide_rels_xml(target: &Target, default_rels: &[(&str, &str)]) -> String {
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>{CRLF}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">"
    );
    let mut last = 0u64;
    for rel in &target.rels {
        last = last.max(rel.r_id);
        if rel.kind.to_ascii_lowercase().contains("hyperlink") {
            if rel.data.as_deref() == Some("slide") {
                out.push_str(&format!(
                    "<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide\" Target=\"slide{}.xml\"/>",
                    rel.r_id, rel.target
                ));
            } else {
                out.push_str(&format!(
                    "<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"{}\" TargetMode=\"External\"/>",
                    rel.r_id, rel.target
                ));
            }
        }
    }
    for rel in &target.media {
        last = last.max(rel.r_id);
        if rel.type_.to_ascii_lowercase().contains("image") {
            out.push_str(&format!(
                "<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"{}\"/>",
                rel.r_id, rel.target
            ));
        }
    }
    for (idx, (ty, target_name)) in default_rels.iter().enumerate() {
        out.push_str(&format!(
            "<Relationship Id=\"rId{}\" Type=\"{ty}\" Target=\"{target_name}\"/>",
            last + idx as u64 + 1
        ));
    }
    out.push_str("</Relationships>");
    out
}

pub(crate) fn make_xml_layout_rel(layout: &Target) -> String {
    slide_rels_xml(
        layout,
        &[(
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster",
            "../slideMasters/slideMaster1.xml",
        )],
    )
}

pub(crate) fn make_xml_slide_rel(slide: &Target, _deck: &Deck, slide_number: usize) -> String {
    let layout_idx = slide.slide_layout.unwrap_or(0) + 1;
    slide_rels_xml(
        slide,
        &[
            (
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout",
                &format!("../slideLayouts/slideLayout{layout_idx}.xml"),
            ),
            (
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide",
                &format!("../notesSlides/notesSlide{slide_number}.xml"),
            ),
        ],
    )
}

pub(crate) fn make_xml_notes_slide_rel(slide_number: usize) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\t\t<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\n\t\t\t<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesMaster\" Target=\"../notesMasters/notesMaster1.xml\"/>\n\t\t\t<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide\" Target=\"../slides/slide{slide_number}.xml\"/>\n\t\t</Relationships>"
    )
}

pub(crate) fn make_xml_notes_slide(slide: &Target) -> String {
    let notes = slide
        .objects
        .iter()
        .filter(|o| o.kind == ObjKind::Notes)
        .filter_map(|o| o.text.as_ref())
        .filter_map(|t| t.first())
        .map(|r| r.text.clone())
        .collect::<String>()
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\r\n");
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>{CRLF}<p:notes xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr><p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Slide Image Placeholder 1\"/><p:cNvSpPr><a:spLocks noGrp=\"1\" noRot=\"1\" noChangeAspect=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"sldImg\"/></p:nvPr></p:nvSpPr><p:spPr/></p:sp><p:sp><p:nvSpPr><p:cNvPr id=\"3\" name=\"Notes Placeholder 2\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"body\" idx=\"1\"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang=\"en-US\" dirty=\"0\"/><a:t>{}</a:t></a:r><a:endParaRPr lang=\"en-US\" dirty=\"0\"/></a:p></p:txBody></p:sp><p:sp><p:nvSpPr><p:cNvPr id=\"4\" name=\"Slide Number Placeholder 3\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"sldNum\" sz=\"quarter\" idx=\"10\"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:fld id=\"{SLDNUMFLDID}\" type=\"slidenum\"><a:rPr lang=\"en-US\"/><a:t>‹#›</a:t></a:fld><a:endParaRPr lang=\"en-US\"/></a:p></p:txBody></p:sp></p:spTree><p:extLst><p:ext uri=\"{{BB962C8B-B14F-4D97-AF65-F5344CB8AC3E}}\"><p14:creationId xmlns:p14=\"http://schemas.microsoft.com/office/powerpoint/2010/main\" val=\"1024086991\"/></p:ext></p:extLst></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:notes>",
        encode_xml_str(&notes),
    )
}

pub(crate) fn make_xml_content_types(deck: &Deck) -> String {
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>{CRLF}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">"
    );
    out.push_str("<Default Extension=\"xml\" ContentType=\"application/xml\"/>");
    out.push_str("<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>");
    out.push_str("<Default Extension=\"jpeg\" ContentType=\"image/jpeg\"/>");
    out.push_str("<Default Extension=\"jpg\" ContentType=\"image/jpg\"/>");
    out.push_str("<Default Extension=\"svg\" ContentType=\"image/svg+xml\"/>");
    out.push_str("<Default Extension=\"png\" ContentType=\"image/png\"/>");
    out.push_str("<Default Extension=\"gif\" ContentType=\"image/gif\"/>");
    out.push_str("<Default Extension=\"m4v\" ContentType=\"video/mp4\"/>");
    out.push_str("<Default Extension=\"mp4\" ContentType=\"video/mp4\"/>");
    for slide in &deck.slides {
        for rel in &slide.media {
            if rel.type_ != "image"
                && rel.type_ != "online"
                && rel.type_ != "chart"
                && rel.extn != "m4v"
                && !out.contains(&rel.type_)
            {
                out.push_str(&format!(
                    "<Default Extension=\"{}\" ContentType=\"{}\"/>",
                    rel.extn, rel.type_
                ));
            }
        }
    }
    out.push_str("<Default Extension=\"vml\" ContentType=\"application/vnd.openxmlformats-officedocument.vmlDrawing\"/>");
    out.push_str("<Default Extension=\"xlsx\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet\"/>");
    out.push_str("<Override PartName=\"/ppt/presentation.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml\"/>");
    out.push_str("<Override PartName=\"/ppt/notesMasters/notesMaster1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.notesMaster+xml\"/>");
    for (idx, _slide) in deck.slides.iter().enumerate() {
        out.push_str(&format!(
            "<Override PartName=\"/ppt/slideMasters/slideMaster{}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml\"/>",
            idx + 1
        ));
        out.push_str(&format!(
            "<Override PartName=\"/ppt/slides/slide{}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>",
            idx + 1
        ));
    }
    out.push_str("<Override PartName=\"/ppt/presProps.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.presProps+xml\"/>");
    out.push_str("<Override PartName=\"/ppt/viewProps.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.viewProps+xml\"/>");
    out.push_str("<Override PartName=\"/ppt/theme/theme1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>");
    out.push_str("<Override PartName=\"/ppt/tableStyles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.tableStyles+xml\"/>");
    for (idx, _layout) in deck.layouts.iter().enumerate() {
        out.push_str(&format!(
            "<Override PartName=\"/ppt/slideLayouts/slideLayout{}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml\"/>",
            idx + 1
        ));
    }
    for idx in 0..deck.slides.len() {
        out.push_str(&format!(
            "<Override PartName=\"/ppt/notesSlides/notesSlide{}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml\"/>",
            idx + 1
        ));
    }
    for rel in &deck.master.media {
        if rel.type_ != "image"
            && rel.type_ != "online"
            && rel.type_ != "chart"
            && rel.extn != "m4v"
            && !out.contains(&rel.type_)
        {
            out.push_str(&format!(
                " <Default Extension=\"{}\" ContentType=\"{}\"/>",
                rel.extn, rel.type_
            ));
        }
    }
    out.push_str(" <Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>");
    out.push_str(" <Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/>");
    out.push_str("</Types>");
    out
}

pub(crate) fn make_xml_app(deck: &Deck) -> String {
    let n = deck.slides.len();
    let mut titles = String::new();
    for idx in 0..n {
        titles.push_str(&format!("<vt:lpstr>Slide {}</vt:lpstr>", idx + 1));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>{CRLF}<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\" xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\">\n\t<TotalTime>0</TotalTime>\n\t<Words>0</Words>\n\t<Application>Microsoft Office PowerPoint</Application>\n\t<PresentationFormat>On-screen Show (16:9)</PresentationFormat>\n\t<Paragraphs>0</Paragraphs>\n\t<Slides>{n}</Slides>\n\t<Notes>{n}</Notes>\n\t<HiddenSlides>0</HiddenSlides>\n\t<MMClips>0</MMClips>\n\t<ScaleCrop>false</ScaleCrop>\n\t<HeadingPairs>\n\t\t<vt:vector size=\"6\" baseType=\"variant\">\n\t\t\t<vt:variant><vt:lpstr>Fonts Used</vt:lpstr></vt:variant>\n\t\t\t<vt:variant><vt:i4>2</vt:i4></vt:variant>\n\t\t\t<vt:variant><vt:lpstr>Theme</vt:lpstr></vt:variant>\n\t\t\t<vt:variant><vt:i4>1</vt:i4></vt:variant>\n\t\t\t<vt:variant><vt:lpstr>Slide Titles</vt:lpstr></vt:variant>\n\t\t\t<vt:variant><vt:i4>{n}</vt:i4></vt:variant>\n\t\t</vt:vector>\n\t</HeadingPairs>\n\t<TitlesOfParts>\n\t\t<vt:vector size=\"{}\" baseType=\"lpstr\">\n\t\t\t<vt:lpstr>Arial</vt:lpstr>\n\t\t\t<vt:lpstr>Calibri</vt:lpstr>\n\t\t\t<vt:lpstr>Office Theme</vt:lpstr>\n\t\t\t{titles}\n\t\t</vt:vector>\n\t</TitlesOfParts>\n\t<Company>Gwen</Company>\n\t<LinksUpToDate>false</LinksUpToDate>\n\t<SharedDoc>false</SharedDoc>\n\t<HyperlinksChanged>false</HyperlinksChanged>\n\t<AppVersion>16.0000</AppVersion>\n\t</Properties>",
        n + 3
    )
}

pub(crate) fn make_xml_core() -> String {
    let now = iso8601_now();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\t<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:dcmitype=\"http://purl.org/dc/dcmitype/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\n\t\t<dc:title>Gwen Presentation</dc:title>\n\t\t<dc:subject>Gwen Presentation</dc:subject>\n\t\t<dc:creator>Gwen</dc:creator>\n\t\t<cp:lastModifiedBy>Gwen</cp:lastModifiedBy>\n\t\t<cp:revision>1</cp:revision>\n\t\t<dcterms:created xsi:type=\"dcterms:W3CDTF\">{now}</dcterms:created>\n\t\t<dcterms:modified xsi:type=\"dcterms:W3CDTF\">{now}</dcterms:modified>\n\t</cp:coreProperties>"
    )
}

/// Current UTC time in the W3CDTF form core properties expect.
fn iso8601_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (hour, minute, second) = (rem / 3_600, (rem % 3_600) / 60, rem % 60);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

pub(crate) fn make_xml_presentation_rels(deck: &Deck) -> String {
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>{CRLF}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">"
    );
    out.push_str("<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster\" Target=\"slideMasters/slideMaster1.xml\"/>");
    let mut rel = 1u64;
    for idx in 1..=deck.slides.len() {
        rel += 1;
        out.push_str(&format!(
            "<Relationship Id=\"rId{rel}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide\" Target=\"slides/slide{idx}.xml\"/>"
        ));
    }
    rel += 1;
    out.push_str(&format!(
        "<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesMaster\" Target=\"notesMasters/notesMaster1.xml\"/>",
        rel
    ));
    out.push_str(&format!(
        "<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/presProps\" Target=\"presProps.xml\"/>",
        rel + 1
    ));
    out.push_str(&format!(
        "<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/viewProps\" Target=\"viewProps.xml\"/>",
        rel + 2
    ));
    out.push_str(&format!(
        "<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme\" Target=\"theme/theme1.xml\"/>",
        rel + 3
    ));
    out.push_str(&format!(
        "<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/tableStyles\" Target=\"tableStyles.xml\"/>",
        rel + 4
    ));
    out.push_str("</Relationships>");
    out
}

pub(crate) fn theme_xml(major_font: &str, minor_font: &str) -> String {
    let major = if major_font.is_empty() {
        "<a:latin typeface=\"Calibri Light\" panose=\"020F0302020204030204\"/>".to_string()
    } else {
        format!("<a:latin typeface=\"{major_font}\"/>")
    };
    let minor = if minor_font.is_empty() {
        "<a:latin typeface=\"Calibri\" panose=\"020F0502020204030204\"/>".to_string()
    } else {
        format!("<a:latin typeface=\"{minor_font}\"/>")
    };
    String::from_utf8_lossy(include_bytes!("static/theme.xml"))
        .replace("{{MAJOR_LATIN}}", &major)
        .replace("{{MINOR_LATIN}}", &minor)
}

pub(crate) fn make_xml_theme(deck: &Deck) -> String {
    theme_xml(&deck.major_font, &deck.minor_font)
}

pub(crate) fn make_xml_presentation(deck: &Deck) -> String {
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>{CRLF}<p:presentation xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"  saveSubsetFonts=\"1\" autoCompressPictures=\"0\">"
    );
    out.push_str(
        "<p:sldMasterIdLst><p:sldMasterId id=\"2147483648\" r:id=\"rId1\"/></p:sldMasterIdLst>",
    );
    out.push_str("<p:sldIdLst>");
    for (idx, _slide) in deck.slides.iter().enumerate() {
        out.push_str(&format!(
            "<p:sldId id=\"{}\" r:id=\"rId{}\"/>",
            idx + 256,
            idx + 2
        ));
    }
    out.push_str("</p:sldIdLst>");
    out.push_str(&format!(
        "<p:notesMasterIdLst><p:notesMasterId r:id=\"rId{}\"/></p:notesMasterIdLst>",
        deck.slides.len() + 2
    ));
    out.push_str(&format!(
        "<p:sldSz cx=\"{}\" cy=\"{}\"/>",
        deck.width as i64, deck.height as i64
    ));
    out.push_str(&format!(
        "<p:notesSz cx=\"{}\" cy=\"{}\"/>",
        deck.height as i64, deck.width as i64
    ));
    out.push_str("<p:defaultTextStyle>");
    for idy in 1..10 {
        out.push_str(&format!(
            "<a:lvl{idy}pPr marL=\"{}\" algn=\"l\" defTabSz=\"914400\" rtl=\"0\" eaLnBrk=\"1\" latinLnBrk=\"0\" hangingPunct=\"1\"><a:defRPr sz=\"1800\" kern=\"1200\"><a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill><a:latin typeface=\"+mn-lt\"/><a:ea typeface=\"+mn-ea\"/><a:cs typeface=\"+mn-cs\"/></a:defRPr></a:lvl{idy}pPr>",
            (idy - 1) * 457200
        ));
    }
    out.push_str("</p:defaultTextStyle>");
    if !deck.sections.is_empty() {
        out.push_str("<p:extLst><p:ext uri=\"{521415D9-36F7-43E2-AB2F-B90AF26B5E84}\">");
        out.push_str("<p14:sectionLst xmlns:p14=\"http://schemas.microsoft.com/office/powerpoint/2010/main\">");
        for (idx, section) in deck.sections.iter().enumerate() {
            out.push_str(&format!(
                "<p14:section name=\"{}\" id=\"{{{}}}\"><p14:sldIdLst>",
                encode_xml_str(&section.title),
                deck.section_uuids.get(idx).cloned().unwrap_or_default()
            ));
            for slide_id in &section.slide_ids {
                out.push_str(&format!("<p14:sldId id=\"{}\"/>", slide_id + 256));
            }
            out.push_str("</p14:sldIdLst></p14:section>");
        }
        out.push_str("</p14:sectionLst></p:ext>");
        out.push_str("<p:ext uri=\"{EFAFB233-063F-42B5-8137-9DF3F51BA10A}\"><p15:sldGuideLst xmlns:p15=\"http://schemas.microsoft.com/office/powerpoint/2012/main\"/></p:ext>");
        out.push_str("</p:extLst>");
    }
    out.push_str("</p:presentation>");
    out
}

pub(crate) fn make_xml_master(master: &Target, deck: &Deck) -> String {
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>{CRLF}<p:sldMaster xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\">"
    );
    out.push_str(&slide_object_xml(master, deck, &[]));
    out.push_str("{{TAIL}}");
    out
}

pub(crate) fn master_layout_ids(master: &Target, layouts: &[Target]) -> String {
    let mut out = String::new();
    for (idx, _layout) in layouts.iter().enumerate() {
        out.push_str(&format!(
            "<p:sldLayoutId id=\"{}\" r:id=\"rId{}\"/>",
            LAYOUT_IDX_SERIES_BASE + idx as u64,
            master.rels.len() + idx + 1
        ));
    }
    format!("<p:sldLayoutIdLst>{out}</p:sldLayoutIdLst>")
}

pub(crate) fn make_xml_master_rel(master: &Target, layouts: &[Target]) -> String {
    let mut defaults: Vec<(String, String)> = layouts
        .iter()
        .enumerate()
        .map(|(idx, _)| {
            (
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout"
                    .to_string(),
                format!("../slideLayouts/slideLayout{}.xml", idx + 1),
            )
        })
        .collect();
    defaults.push((
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme".to_string(),
        "../theme/theme1.xml".to_string(),
    ));
    let refs: Vec<(&str, &str)> = defaults
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    slide_rels_xml(master, &refs)
}

pub(crate) fn finish_master(xml: String) -> String {
    let tail = String::from_utf8_lossy(include_bytes!("static/master_tail.xml"));
    xml.replace("{{TAIL}}", &tail)
}
