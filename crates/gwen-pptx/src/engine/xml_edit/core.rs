use super::*;

/// Map a snake_case core-property name to its element tag.
fn core_prop_tag(name: &str) -> Option<&'static str> {
    match name {
        "title" => Some("dc:title"),
        "subject" => Some("dc:subject"),
        "author" => Some("dc:creator"),
        "keywords" => Some("cp:keywords"),
        "comments" => Some("dc:description"),
        "last_modified_by" => Some("cp:lastModifiedBy"),
        "revision" => Some("cp:revision"),
        "created" => Some("dcterms:created"),
        "modified" => Some("dcterms:modified"),
        "category" => Some("cp:category"),
        "content_status" => Some("cp:contentStatus"),
        _ => None,
    }
}

/// Map an element tag to its snake_case core-property name.
pub fn core_prop_key(tag: &str) -> Option<&'static str> {
    match tag {
        "dc:title" => Some("title"),
        "dc:subject" => Some("subject"),
        "dc:creator" => Some("author"),
        "cp:keywords" => Some("keywords"),
        "dc:description" => Some("comments"),
        "cp:lastModifiedBy" => Some("last_modified_by"),
        "cp:revision" => Some("revision"),
        "dcterms:created" => Some("created"),
        "dcterms:modified" => Some("modified"),
        "cp:category" => Some("category"),
        "cp:contentStatus" => Some("content_status"),
        _ => None,
    }
}

/// Set the text of a core property in core.xml, creating the element if it
/// is missing. The value is escaped for XML text content.
pub fn replace_core_property(xml_bytes: &[u8], name: &str, value: &str) -> AppResult<Vec<u8>> {
    let tag = core_prop_tag(name)
        .ok_or_else(|| AppError::PathParse(format!("Unknown core property '{name}'")))?;

    let mut events = read_events(xml_bytes)?;

    // Find the cp:coreProperties root and its closing element.
    let mut root_start: Option<usize> = None;
    for (i, ev) in events.iter().enumerate() {
        if let Event::Start(e) = ev
            && e.name().as_ref() == b"cp:coreProperties"
        {
            root_start = Some(i);
            break;
        }
    }
    let root_start =
        root_start.ok_or_else(|| AppError::PathParse("No cp:coreProperties root".to_string()))?;

    let mut root_end: Option<usize> = None;
    {
        let mut depth = 0u32;
        for (j, ev2) in events.iter().enumerate().skip(root_start) {
            match ev2 {
                Event::Start(_) => depth += 1,
                Event::End(e2) => {
                    depth = depth.saturating_sub(1);
                    if e2.name().as_ref() == b"cp:coreProperties" && depth == 0 {
                        root_end = Some(j);
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    let root_end = root_end
        .ok_or_else(|| AppError::PathParse("No cp:coreProperties root found".to_string()))?;

    // Locate the property element (self-closing Empty, or Start..End).
    let mut prop: Option<(usize, usize, bool)> = None;
    {
        let mut depth = 0u32;
        let mut start: Option<usize> = None;
        for j in (root_start + 1)..root_end {
            match &events[j] {
                Event::Start(e) => {
                    if depth == 0 && e.name().as_ref() == tag.as_bytes() {
                        start = Some(j);
                    }
                    depth += 1;
                }
                Event::Empty(e) if depth == 0 && e.name().as_ref() == tag.as_bytes() => {
                    prop = Some((j, j, true));
                    break;
                }
                Event::End(e) => {
                    if let (Some(s), _) = (start, events.get(j))
                        && depth == 1
                        && e.name().as_ref() == tag.as_bytes()
                    {
                        prop = Some((s, j, false));
                        break;
                    }
                    depth = depth.saturating_sub(1);
                }
                _ => {}
            }
        }
    }

    let text_event = |value: &str| BytesText::from_escaped(escape(value.to_string()));

    match prop {
        Some((start, _end, true)) => {
            // Self-closing element -> expand to start + text + end.
            let Event::Empty(e) = events.remove(start) else {
                unreachable!()
            };
            let name_str = String::from_utf8_lossy(e.name().as_ref()).to_string();
            let mut start_elem = BytesStart::new(name_str.clone());
            for a in e.attributes().flatten() {
                start_elem.push_attribute((a.key.as_ref(), a.value.as_ref()));
            }
            events.insert(start, Event::End(BytesEnd::new(name_str)));
            events.insert(start, Event::Text(text_event(value)));
            events.insert(start, Event::Start(start_elem));
        }
        Some((start, end, false)) => {
            // Replace the text content directly inside the element, inserting
            // it if the element is currently empty.
            replace_text_in_range(&mut events, start, end, value);
        }
        None => {
            // Insert a new element just before the root's closing tag.
            events.insert(root_end, Event::End(BytesEnd::new(tag.to_string())));
            events.insert(root_end, Event::Text(text_event(value)));
            events.insert(root_end, Event::Start(BytesStart::new(tag.to_string())));
        }
    }

    write_events(events)
}

const BG_FILL_TAGS: [&[u8]; 7] = [
    b"a:solidFill",
    b"a:noFill",
    b"a:gradFill",
    b"a:blipFill",
    b"a:pattFill",
    b"a:grpFill",
    b"a:bgFill",
];

pub fn set_slide_background(xml_bytes: &[u8], color: &str) -> AppResult<Vec<u8>> {
    let mut events = read_events(xml_bytes)?;

    let csld_start = events
        .iter()
        .position(|e| matches!(e, Event::Start(ev) if ev.name().as_ref() == b"p:cSld"))
        .ok_or_else(|| AppError::PathParse("No p:cSld found in slide".to_string()))?;

    let fill_events = color_xml_events(color);

    let bg_range = find_elem_range(&events, b"p:bg", csld_start);

    if let Some((bs, be)) = bg_range {
        let bgpr = find_elem_range(&events, b"p:bgPr", bs).filter(|r| r.0 <= be);
        match bgpr {
            Some((ps, pe)) => {
                remove_children_by_name(&mut events, ps, pe, &BG_FILL_TAGS);
                for (j, ev) in fill_events.into_iter().enumerate() {
                    events.insert(ps + 1 + j, ev);
                }
            }
            None => {
                remove_children_by_name(&mut events, bs, be, &[b"p:bgRef"]);
                events.insert(bs + 1, Event::End(BytesEnd::new("p:bgPr")));
                for ev in fill_events.into_iter().rev() {
                    events.insert(bs + 1, ev);
                }
                events.insert(bs + 1, Event::Start(BytesStart::new("p:bgPr")));
            }
        }
    } else {
        events.insert(csld_start + 1, Event::End(BytesEnd::new("p:bg")));
        events.insert(csld_start + 1, Event::End(BytesEnd::new("p:bgPr")));
        for ev in fill_events.into_iter().rev() {
            events.insert(csld_start + 1, ev);
        }
        events.insert(csld_start + 1, Event::Start(BytesStart::new("p:bgPr")));
        events.insert(csld_start + 1, Event::Start(BytesStart::new("p:bg")));
    }

    write_events(events)
}

pub fn parse_slide_background(xml_bytes: &[u8]) -> serde_json::Value {
    let events = match read_events(xml_bytes) {
        Ok(e) => e,
        Err(_) => return serde_json::Value::Null,
    };
    let csld_start = match events
        .iter()
        .position(|e| matches!(e, Event::Start(ev) if ev.name().as_ref() == b"p:cSld"))
    {
        Some(i) => i,
        None => return serde_json::Value::Null,
    };
    let Some((bs, be)) = find_elem_range(&events, b"p:bg", csld_start) else {
        return json!({ "fill": { "type": serde_json::Value::Null } });
    };
    if let Some((ps, pe)) = find_elem_range(&events, b"p:bgPr", bs).filter(|r| r.0 <= be) {
        if let Some((fs, fe)) = find_elem_range(&events, b"a:solidFill", ps).filter(|r| r.0 <= pe) {
            let color = (fs..=fe)
                .find(|&i| {
                    matches!(&events[i], Event::Empty(e) | Event::Start(e) if e.name().as_ref() == b"a:srgbClr")
                })
                .and_then(|i| {
                    let (Event::Empty(e) | Event::Start(e)) = &events[i] else {
                        return None;
                    };
                    e.attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == b"val")
                        .map(|a| String::from_utf8_lossy(&a.value).to_string())
                });
            return json!({
                "fill": {
                    "type": "SOLID",
                    "color": color.unwrap_or_default(),
                }
            });
        }
        return json!({ "fill": { "type": "GRADIENT" } });
    }
    if find_elem_range(&events, b"p:bgRef", bs).is_some_and(|r| r.0 <= be) {
        return json!({ "fill": { "type": "THEME" } });
    }
    json!({ "fill": { "type": serde_json::Value::Null } })
}

pub const THEME_COLOR_NAMES: [&str; 12] = [
    "dk1", "lt1", "dk2", "lt2", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6",
    "hlink", "folHlink",
];

/// Replace a theme color (`p.theme.colors.accent1`) or font
/// (`p.theme.fonts.major` / `.minor`). `remaining` is the segments after
/// `theme`.
pub fn replace_theme_property(
    xml_bytes: &[u8],
    remaining: &[path::PathSegment],
    value: &str,
) -> AppResult<Vec<u8>> {
    let mut events = read_events(xml_bytes)?;

    match remaining {
        [path::PathSegment::Field(n), path::PathSegment::Field(prop)]
            if n == "colors" && THEME_COLOR_NAMES.contains(&prop.as_str()) =>
        {
            let clr_range = find_elem_range(&events, b"a:clrScheme", 0)
                .ok_or_else(|| AppError::PathParse("No color scheme in theme".to_string()))?;
            let child_name = format!("a:{prop}");
            let child =
                find_child_elem_range(&events, clr_range.0, clr_range.1, child_name.as_bytes())
                    .ok_or_else(|| {
                        AppError::PathParse(format!("Theme color '{prop}' not found"))
                    })?;
            let srgb = find_child_elem_range(&events, child.0, child.1, b"a:srgbClr");
            match srgb {
                Some((s, e)) => set_srgb_attr(&mut events, s, e, value),
                None => {
                    let mut clr = BytesStart::new("a:srgbClr");
                    clr.push_attribute(("val", value));
                    events.insert(child.0 + 1, Event::Empty(clr));
                }
            }
        }
        [path::PathSegment::Field(n), path::PathSegment::Field(prop)]
            if n == "fonts" && matches!(prop.as_str(), "major" | "minor") =>
        {
            let font_range = find_elem_range(&events, b"a:fontScheme", 0)
                .ok_or_else(|| AppError::PathParse("No font scheme in theme".to_string()))?;
            let family = if prop == "major" {
                b"a:majorFont"
            } else {
                b"a:minorFont"
            };
            let family_range =
                find_child_elem_range(&events, font_range.0, font_range.1, family)
                    .ok_or_else(|| AppError::PathParse("Font family not found".to_string()))?;
            let latin = find_child_elem_range(&events, family_range.0, family_range.1, b"a:latin");
            match latin {
                Some((s, e)) => set_latin_typeface(&mut events, s, e, value),
                None => {
                    let mut latin = BytesStart::new("a:latin");
                    latin.push_attribute(("typeface", value));
                    events.insert(family_range.0 + 1, Event::Empty(latin));
                }
            }
        }
        _ => {
            return Err(AppError::PathParse(
                "Expected theme.colors.<name> or theme.fonts.major/minor".to_string(),
            ));
        }
    }

    write_events(events)
}

/// Remove a core document property element from core.xml (a field set to
/// `null` in the overlay).
pub fn delete_core_property(xml_bytes: &[u8], name: &str) -> AppResult<Vec<u8>> {
    let tag = core_prop_tag(name)
        .ok_or_else(|| AppError::PathParse(format!("Unknown core property '{name}'")))?;

    let mut events = read_events(xml_bytes)?;
    let root_start = events
        .iter()
        .position(|e| matches!(e, Event::Start(ev) if ev.name().as_ref() == b"cp:coreProperties"))
        .ok_or_else(|| AppError::PathParse("No cp:coreProperties root".to_string()))?;
    let mut root_end: Option<usize> = None;
    {
        let mut depth = 0u32;
        for (j, ev2) in events.iter().enumerate().skip(root_start) {
            match ev2 {
                Event::Start(_) => depth += 1,
                Event::End(e2) => {
                    depth = depth.saturating_sub(1);
                    if e2.name().as_ref() == b"cp:coreProperties" && depth == 0 {
                        root_end = Some(j);
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    let root_end = root_end
        .ok_or_else(|| AppError::PathParse("No cp:coreProperties root found".to_string()))?;

    let tag_bytes = tag.as_bytes();
    let mut found: Option<(usize, usize)> = None;
    {
        let mut depth = 0u32;
        let mut start: Option<usize> = None;
        for (j, ev) in events
            .iter()
            .enumerate()
            .skip(root_start + 1)
            .take(root_end - root_start - 1)
        {
            match ev {
                Event::Start(e) => {
                    if depth == 0 && e.name().as_ref() == tag_bytes {
                        start = Some(j);
                    }
                    depth += 1;
                }
                Event::Empty(e) if depth == 0 && e.name().as_ref() == tag_bytes => {
                    found = Some((j, j));
                    break;
                }
                Event::End(e) => {
                    if let Some(s) = start
                        && depth == 1
                        && e.name().as_ref() == tag_bytes
                    {
                        found = Some((s, j));
                        break;
                    }
                    depth = depth.saturating_sub(1);
                }
                _ => {}
            }
        }
    }

    match found {
        Some((s, e)) => {
            for j in (s..=e).rev() {
                events.remove(j);
            }
            Ok(write_events(events)?)
        }
        None => Ok(xml_bytes.to_vec()),
    }
}
