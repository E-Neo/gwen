use std::collections::HashMap;
use std::io::Write;

use quick_xml::Reader;
use quick_xml::Writer;
use quick_xml::events::{BytesStart, BytesText, Event};

use crate::dto::ShapeDto;
use crate::error::{AppError, AppResult};
use crate::path;

pub fn replace_text(xml_bytes: &[u8], shape_idx: usize, new_text: &str) -> AppResult<Vec<u8>> {
    let mut reader = Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true);
    let mut writer = Writer::new(Vec::new());
    let mut shape_counter = 0;
    let mut inside_target_shape = false;
    let mut buffer = Vec::new();

    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(ref e)) => {
                if e.name().as_ref() == b"p:sp"
                    || e.name().as_ref() == b"p:pic"
                    || e.name().as_ref() == b"p:graphicFrame"
                {
                    if shape_counter == shape_idx {
                        inside_target_shape = true;
                    }
                    shape_counter += 1;
                }
                writer
                    .write_event(Event::Start(e.clone()))
                    .map_err(AppError::Io)?;

                if inside_target_shape && e.name().as_ref() == b"a:t" {
                    writer
                        .write_event(Event::Text(BytesText::new(new_text)))
                        .map_err(AppError::Io)?;
                    loop {
                        match reader.read_event_into(&mut buffer) {
                            Ok(Event::End(ref end)) if end.name().as_ref() == b"a:t" => {
                                writer
                                    .write_event(Event::End(end.clone()))
                                    .map_err(AppError::Io)?;
                                break;
                            }
                            Ok(Event::Eof) => break,
                            Err(e) => return Err(AppError::Xml(e)),
                            _ => {}
                        }
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                if inside_target_shape
                    && (e.name().as_ref() == b"p:sp" || e.name().as_ref() == b"p:pic")
                {
                    inside_target_shape = false;
                }
                writer
                    .write_event(Event::End(e.clone()))
                    .map_err(AppError::Io)?;
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(AppError::Xml(e)),
            Ok(e) => {
                writer.write_event(e).map_err(AppError::Io)?;
            }
        }
    }
    Ok(writer.into_inner())
}

pub fn replace_shape_text(
    xml_bytes: &[u8],
    shape_idx: usize,
    target: &str,
    value: &str,
) -> AppResult<Vec<u8>> {
    match target {
        "text" => replace_text(xml_bytes, shape_idx, value),
        _ => Err(AppError::InvalidValue(format!(
            "Unsupported target for replace: {:?}",
            target
        ))),
    }
}

pub fn remove_shape(xml_bytes: &[u8], shape_idx: usize) -> AppResult<Vec<u8>> {
    let mut reader = Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true);
    let mut writer = Writer::new(Vec::new());
    let mut shape_counter = 0;
    let mut skip_depth: Option<usize> = None;
    let mut buffer = Vec::new();

    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(ref e)) => {
                let is_shape = e.name().as_ref() == b"p:sp"
                    || e.name().as_ref() == b"p:pic"
                    || e.name().as_ref() == b"p:graphicFrame";
                if is_shape && shape_counter == shape_idx {
                    skip_depth = Some(1);
                    shape_counter += 1;
                    continue;
                }
                if is_shape {
                    shape_counter += 1;
                }

                if let Some(ref mut depth) = skip_depth {
                    *depth += 1;
                    continue;
                }
                writer
                    .write_event(Event::Start(e.clone()))
                    .map_err(AppError::Io)?;
            }
            Ok(Event::End(ref e)) => {
                if let Some(ref mut depth) = skip_depth {
                    *depth -= 1;
                    if *depth == 0 {
                        skip_depth = None;
                    }
                    continue;
                }
                writer
                    .write_event(Event::End(e.clone()))
                    .map_err(AppError::Io)?;
            }
            Ok(Event::Empty(ref e)) => {
                if skip_depth.is_some() {
                    continue;
                }
                writer
                    .write_event(Event::Empty(e.clone()))
                    .map_err(AppError::Io)?;
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(AppError::Xml(e)),
            Ok(e) => {
                if skip_depth.is_none() {
                    writer.write_event(e).map_err(AppError::Io)?;
                }
            }
        }
    }
    Ok(writer.into_inner())
}

pub fn extract_shape_subtree(xml_bytes: &[u8], shape_idx: usize) -> AppResult<(Vec<u8>, u32)> {
    let mut reader = Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true);
    let mut shape_counter = 0;
    let mut depth: Option<usize> = None;
    let mut subtree = Vec::new();
    let mut id = 0u32;
    let mut found = false;
    let mut buffer = Vec::new();

    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(ref e)) => {
                let is_shape = e.name().as_ref() == b"p:sp"
                    || e.name().as_ref() == b"p:pic"
                    || e.name().as_ref() == b"p:graphicFrame";
                if is_shape && shape_counter == shape_idx {
                    depth = Some(1);
                    found = true;
                    shape_counter += 1;

                    let mut ev_buf = Vec::new();
                    ev_buf.extend_from_slice(b"<");
                    ev_buf.extend_from_slice(e.name().as_ref());
                    for a in e.attributes().flatten() {
                        ev_buf.push(b' ');
                        ev_buf.extend_from_slice(a.key.as_ref());
                        ev_buf.extend_from_slice(b"=\"");
                        ev_buf.extend_from_slice(&a.value);
                        ev_buf.push(b'"');
                        if a.key.as_ref() == b"id"
                            && let Ok(v) = String::from_utf8_lossy(&a.value).parse::<u32>()
                        {
                            id = v;
                        }
                    }
                    ev_buf.extend_from_slice(b">");
                    subtree.extend_from_slice(&ev_buf);
                    continue;
                }
                if is_shape {
                    shape_counter += 1;
                }

                if let Some(ref mut d) = depth {
                    *d += 1;
                    let mut ev_buf = Vec::new();
                    ev_buf.extend_from_slice(b"<");
                    ev_buf.extend_from_slice(e.name().as_ref());
                    for a in e.attributes().flatten() {
                        ev_buf.push(b' ');
                        ev_buf.extend_from_slice(a.key.as_ref());
                        ev_buf.extend_from_slice(b"=\"");
                        ev_buf.extend_from_slice(&a.value);
                        ev_buf.push(b'"');
                    }
                    ev_buf.extend_from_slice(b">");
                    subtree.extend_from_slice(&ev_buf);
                }
            }
            Ok(Event::End(ref e)) => {
                if let Some(ref mut d) = depth {
                    subtree.extend_from_slice(b"</");
                    subtree.extend_from_slice(e.name().as_ref());
                    subtree.extend_from_slice(b">");
                    *d -= 1;
                    if *d == 0 {
                        depth = None;
                    }
                    continue;
                }
            }
            Ok(Event::Empty(ref e)) => {
                if depth.is_some() {
                    let mut ev_buf = Vec::new();
                    ev_buf.extend_from_slice(b"<");
                    ev_buf.extend_from_slice(e.name().as_ref());
                    for a in e.attributes().flatten() {
                        ev_buf.push(b' ');
                        ev_buf.extend_from_slice(a.key.as_ref());
                        ev_buf.extend_from_slice(b"=\"");
                        ev_buf.extend_from_slice(&a.value);
                        ev_buf.push(b'"');
                    }
                    ev_buf.extend_from_slice(b"/>");
                    subtree.extend_from_slice(&ev_buf);
                }
            }
            Ok(Event::Text(ref e)) => {
                if depth.is_some() {
                    subtree.extend_from_slice(e);
                }
            }
            Ok(Event::CData(ref e)) => {
                if depth.is_some() {
                    subtree.extend_from_slice(b"<![CDATA[");
                    subtree.extend_from_slice(e);
                    subtree.extend_from_slice(b"]]>");
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(AppError::Xml(e)),
            _ => {}
        }
    }

    if !found {
        return Err(AppError::ShapeIndexOutOfBounds(shape_idx));
    }

    Ok((subtree, id))
}

pub fn insert_shape_after(
    xml_bytes: &[u8],
    insert_idx: usize,
    new_shape_xml: &[u8],
) -> AppResult<Vec<u8>> {
    let mut reader = Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true);
    let mut writer = Writer::new(Vec::new());
    let mut shape_counter = 0;
    let mut inserted = false;
    let mut buffer = Vec::new();

    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(ref e)) => {
                writer
                    .write_event(Event::Start(e.clone()))
                    .map_err(AppError::Io)?;
                let is_shape = e.name().as_ref() == b"p:sp"
                    || e.name().as_ref() == b"p:pic"
                    || e.name().as_ref() == b"p:graphicFrame";
                if is_shape {
                    if shape_counter == insert_idx {
                        writer
                            .get_mut()
                            .write_all(new_shape_xml)
                            .map_err(AppError::Io)?;
                        inserted = true;
                    }
                    shape_counter += 1;
                }
            }
            Ok(Event::End(ref e)) => {
                let is_sp_tree = e.name().as_ref() == b"p:spTree";
                if is_sp_tree && !inserted {
                    writer
                        .get_mut()
                        .write_all(new_shape_xml)
                        .map_err(AppError::Io)?;
                    inserted = true;
                }
                writer
                    .write_event(Event::End(e.clone()))
                    .map_err(AppError::Io)?;
            }
            Ok(Event::Empty(ref e)) => {
                writer
                    .write_event(Event::Empty(e.clone()))
                    .map_err(AppError::Io)?;
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(AppError::Xml(e)),
            Ok(e) => {
                writer.write_event(e).map_err(AppError::Io)?;
            }
        }
    }
    Ok(writer.into_inner())
}

pub fn replace_txbody(xml_bytes: &[u8], shape_idx: usize, new_txbody: &[u8]) -> AppResult<Vec<u8>> {
    let mut reader = Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true);
    let mut writer = Writer::new(Vec::new());
    let mut shape_counter = 0;
    let mut inside_target = false;
    let mut txbody_depth: Option<usize> = None;
    let mut buffer = Vec::new();

    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(ref e)) => {
                let is_shape = e.name().as_ref() == b"p:sp"
                    || e.name().as_ref() == b"p:pic"
                    || e.name().as_ref() == b"p:graphicFrame";
                if is_shape {
                    if shape_counter == shape_idx {
                        inside_target = true;
                    }
                    shape_counter += 1;
                }

                if inside_target && txbody_depth.is_none() && e.name().as_ref() == b"p:txBody" {
                    writer
                        .write_event(Event::Start(e.clone()))
                        .map_err(AppError::Io)?;
                    writer
                        .get_mut()
                        .write_all(new_txbody)
                        .map_err(AppError::Io)?;
                    txbody_depth = Some(1);
                    continue;
                }

                if let Some(ref mut d) = txbody_depth {
                    *d += 1;
                    continue;
                }

                writer
                    .write_event(Event::Start(e.clone()))
                    .map_err(AppError::Io)?;
            }
            Ok(Event::End(ref e)) => {
                if let Some(ref mut d) = txbody_depth {
                    *d -= 1;
                    if *d == 0 {
                        writer
                            .write_event(Event::End(e.clone()))
                            .map_err(AppError::Io)?;
                        txbody_depth = None;
                        inside_target = false;
                        continue;
                    }
                    continue;
                }

                if inside_target
                    && (e.name().as_ref() == b"p:sp"
                        || e.name().as_ref() == b"p:pic"
                        || e.name().as_ref() == b"p:graphicFrame")
                {
                    inside_target = false;
                }

                writer
                    .write_event(Event::End(e.clone()))
                    .map_err(AppError::Io)?;
            }
            Ok(Event::Empty(ref e)) => {
                if txbody_depth.is_some() {
                    continue;
                }
                writer
                    .write_event(Event::Empty(e.clone()))
                    .map_err(AppError::Io)?;
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(AppError::Xml(e)),
            Ok(e) => {
                if txbody_depth.is_none() {
                    writer.write_event(e).map_err(AppError::Io)?;
                }
            }
        }
    }
    Ok(writer.into_inner())
}

fn build_attr_map(e: &BytesStart) -> Vec<(Vec<u8>, Vec<u8>)> {
    e.attributes()
        .flatten()
        .map(|a| (a.key.as_ref().to_vec(), a.value.as_ref().to_vec()))
        .collect()
}

fn rebuild_elem<'a>(name: &'a str, attrs: &[(Vec<u8>, Vec<u8>)]) -> BytesStart<'a> {
    let mut elem = BytesStart::new(name);
    for (k, v) in attrs {
        elem.push_attribute((k.as_slice(), v.as_slice()));
    }
    elem
}

pub fn replace_shape_attr(
    xml_bytes: &[u8],
    shape_idx: usize,
    remaining: &[path::PathSegment],
    value: &str,
) -> AppResult<Vec<u8>> {
    let attr_name = match remaining.first() {
        Some(path::PathSegment::Field(name)) => name.as_str(),
        _ => return Err(AppError::PathParse("Expected field name".to_string())),
    };

    let (target_tag, target_attr): (&[u8], &[u8]) = match attr_name {
        "name" => (b"p:cNvPr", b"name"),
        "left" => (b"a:off", b"x"),
        "top" => (b"a:off", b"y"),
        "width" => (b"a:ext", b"cx"),
        "height" => (b"a:ext", b"cy"),
        "rotation" => (b"", b"rot"),
        _ => {
            return Err(AppError::PathParse(format!(
                "Unsupported attribute '{attr_name}'"
            )));
        }
    };

    let is_rotation = matches!(attr_name, "rotation");

    let mut reader = Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true);
    let mut writer = Writer::new(Vec::new());
    let mut shape_counter = 0;
    let mut inside_target = false;
    let mut done = false;
    let mut buffer = Vec::new();

    loop {
        if done {
            match reader.read_event_into(&mut buffer) {
                Ok(Event::Eof) => break,
                Err(e) => return Err(AppError::Xml(e)),
                Ok(e) => {
                    writer.write_event(e).map_err(AppError::Io)?;
                }
            }
            continue;
        }

        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(ref e)) => {
                let ename = e.name();
                let ename_bytes = ename.as_ref();
                let is_shape = ename_bytes == b"p:sp"
                    || ename_bytes == b"p:pic"
                    || ename_bytes == b"p:graphicFrame";
                if is_shape {
                    if shape_counter == shape_idx {
                        inside_target = true;
                    }
                    shape_counter += 1;
                }
                let match_elem = if is_rotation {
                    ename_bytes == b"a:xfrm" || ename_bytes == b"p:xfrm"
                } else {
                    ename_bytes == target_tag
                };
                if inside_target && !is_shape && match_elem {
                    let mut attrs = build_attr_map(e);
                    let override_bytes: Vec<u8> = if is_rotation {
                        let rv = value
                            .parse::<f64>()
                            .map(|v| (v * 60000.0).round() as i64)
                            .unwrap_or(0);
                        rv.to_string().into_bytes()
                    } else {
                        value.as_bytes().to_vec()
                    };
                    for (k, v) in &mut attrs {
                        if k.as_slice() == target_attr {
                            *v = override_bytes.clone();
                        }
                    }
                    let name_str = String::from_utf8_lossy(ename_bytes).to_string();
                    let elem = rebuild_elem(&name_str, &attrs);
                    writer
                        .write_event(Event::Start(elem))
                        .map_err(AppError::Io)?;
                    done = true;
                    inside_target = false;
                } else {
                    writer
                        .write_event(Event::Start(e.clone()))
                        .map_err(AppError::Io)?;
                }
            }
            Ok(Event::End(ref e)) => {
                if inside_target
                    && (e.name().as_ref() == b"p:sp"
                        || e.name().as_ref() == b"p:pic"
                        || e.name().as_ref() == b"p:graphicFrame")
                {
                    inside_target = false;
                }
                writer
                    .write_event(Event::End(e.clone()))
                    .map_err(AppError::Io)?;
            }
            Ok(Event::Empty(ref e)) => {
                let ename = e.name();
                let ename_bytes = ename.as_ref();
                let match_elem = if is_rotation {
                    ename_bytes == b"a:xfrm" || ename_bytes == b"p:xfrm"
                } else {
                    ename_bytes == target_tag
                };
                if inside_target && match_elem {
                    let mut attrs = build_attr_map(e);
                    let override_bytes: Vec<u8> = if is_rotation {
                        let rv = value
                            .parse::<f64>()
                            .map(|v| (v * 60000.0).round() as i64)
                            .unwrap_or(0);
                        rv.to_string().into_bytes()
                    } else {
                        value.as_bytes().to_vec()
                    };
                    for (k, v) in &mut attrs {
                        if k.as_slice() == target_attr {
                            *v = override_bytes.clone();
                        }
                    }
                    let name_str = String::from_utf8_lossy(ename_bytes).to_string();
                    let elem = rebuild_elem(&name_str, &attrs);
                    writer
                        .write_event(Event::Empty(elem))
                        .map_err(AppError::Io)?;
                    done = true;
                    inside_target = false;
                    continue;
                }
                writer
                    .write_event(Event::Empty(e.clone()))
                    .map_err(AppError::Io)?;
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(AppError::Xml(e)),
            Ok(e) => {
                writer.write_event(e).map_err(AppError::Io)?;
            }
        }
    }
    Ok(writer.into_inner())
}

pub fn replace_presentation_property(
    xml_bytes: &[u8],
    remaining: &[path::PathSegment],
    value: &str,
) -> AppResult<Vec<u8>> {
    let attr_name = match remaining.first() {
        Some(path::PathSegment::Field(name)) => name.as_str(),
        _ => return Err(AppError::PathParse("Expected field name".to_string())),
    };

    let mut reader = Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true);
    let mut writer = Writer::new(Vec::new());
    let mut buffer = Vec::new();

    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(ref e)) => {
                let ename = e.name();
                let ename_bytes = ename.as_ref();
                if ename_bytes == b"p:sldSz" {
                    let mut attrs = build_attr_map(e);
                    let override_key = if attr_name == "slide_width" {
                        b"cx"
                    } else {
                        b"cy"
                    };
                    let override_bytes = value.as_bytes().to_vec();
                    for (k, v) in &mut attrs {
                        if k.as_slice() == override_key {
                            *v = override_bytes.clone();
                        }
                    }
                    let name_str = String::from_utf8_lossy(ename_bytes).to_string();
                    let elem = rebuild_elem(&name_str, &attrs);
                    writer
                        .write_event(Event::Start(elem))
                        .map_err(AppError::Io)?;
                } else {
                    writer
                        .write_event(Event::Start(e.clone()))
                        .map_err(AppError::Io)?;
                }
            }
            Ok(Event::Empty(ref e)) => {
                let ename = e.name();
                let ename_bytes = ename.as_ref();
                if ename_bytes == b"p:sldSz" {
                    let mut attrs = build_attr_map(e);
                    let override_key = if attr_name == "slide_width" {
                        b"cx"
                    } else {
                        b"cy"
                    };
                    let override_bytes = value.as_bytes().to_vec();
                    for (k, v) in &mut attrs {
                        if k.as_slice() == override_key {
                            *v = override_bytes.clone();
                        }
                    }
                    let name_str = String::from_utf8_lossy(ename_bytes).to_string();
                    let elem = rebuild_elem(&name_str, &attrs);
                    writer
                        .write_event(Event::Empty(elem))
                        .map_err(AppError::Io)?;
                } else {
                    writer
                        .write_event(Event::Empty(e.clone()))
                        .map_err(AppError::Io)?;
                }
            }
            Ok(Event::End(ref e)) => {
                writer
                    .write_event(Event::End(e.clone()))
                    .map_err(AppError::Io)?;
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(AppError::Xml(e)),
            Ok(e) => {
                writer.write_event(e).map_err(AppError::Io)?;
            }
        }
    }
    Ok(writer.into_inner())
}

pub fn replace_shape_property(
    xml_bytes: &[u8],
    shape_idx: usize,
    remaining: &[path::PathSegment],
    value: &str,
) -> AppResult<Vec<u8>> {
    let empty_map = HashMap::new();
    let shapes = crate::model::slide::parse_slide_shapes(xml_bytes, &empty_map)?;

    let shape = shapes
        .get(shape_idx)
        .ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;

    let mut json = serde_json::to_value(shape)
        .map_err(|e| AppError::InvalidValue(format!("Serialization error: {e}")))?;

    let new_value: serde_json::Value = serde_json::from_str(value)
        .map_err(|e| AppError::InvalidValue(format!("Invalid JSON value: {e}")))?;

    {
        let mut current = &mut json;
        for seg in remaining {
            current = match seg {
                path::PathSegment::Field(name) => {
                    if !current.get(name.as_str()).is_some()
                        && let Some(map) = current.as_object_mut()
                    {
                        map.insert(name.clone(), serde_json::Value::Null);
                    }
                    current.get_mut(name.as_str()).ok_or_else(|| {
                        AppError::PathParse(format!("Field '{name}' not found in JSON tree"))
                    })?
                }
                path::PathSegment::Index(idx) => current.get_mut(*idx).ok_or_else(|| {
                    AppError::PathParse(format!("Index {idx} out of bounds in JSON tree"))
                })?,
            };
        }
        *current = new_value;
    }

    let modified_shape: ShapeDto = serde_json::from_value(json)
        .map_err(|e| AppError::InvalidValue(format!("Deserialization error: {e}")))?;

    let new_txbody = match modified_shape.text_frame {
        Some(ref tf) => crate::dto::xml::txbody_to_xml(tf),
        None => {
            return Err(AppError::InvalidValue(
                "Target shape has no text frame".to_string(),
            ));
        }
    };

    replace_txbody(xml_bytes, shape_idx, new_txbody.as_bytes())
}

fn replace_table_tbl(xml_bytes: &[u8], shape_idx: usize, new_tbl: &[u8]) -> AppResult<Vec<u8>> {
    let mut reader = Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true);
    let mut writer = Writer::new(Vec::new());
    let mut shape_counter = 0;
    let mut inside_target = false;
    let mut tbl_depth = 0usize;
    let mut inside_tbl = false;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let en = e.name();
                let enb = en.as_ref();
                let is_shape = enb == b"p:sp" || enb == b"p:pic" || enb == b"p:graphicFrame";
                if is_shape {
                    if shape_counter == shape_idx {
                        inside_target = true;
                    }
                    shape_counter += 1;
                }
                if inside_target && enb == b"a:tbl" {
                    inside_tbl = true;
                    tbl_depth = 1;
                    writer.get_mut().write_all(new_tbl).map_err(AppError::Io)?;
                    continue;
                }
                if inside_tbl {
                    tbl_depth += 1;
                    continue;
                }
                writer
                    .write_event(Event::Start(e.clone()))
                    .map_err(AppError::Io)?;
            }
            Ok(Event::End(ref e)) => {
                let en = e.name();
                let enb = en.as_ref();
                if inside_tbl {
                    tbl_depth = tbl_depth.saturating_sub(1);
                    if tbl_depth == 0 {
                        inside_tbl = false;
                    }
                    continue;
                }
                writer
                    .write_event(Event::End(e.clone()))
                    .map_err(AppError::Io)?;
                if inside_target && (enb == b"p:sp" || enb == b"p:pic" || enb == b"p:graphicFrame")
                {
                    inside_target = false;
                }
            }
            Ok(Event::Empty(ref e)) => {
                if inside_tbl {
                    continue;
                }
                writer
                    .write_event(Event::Empty(e.clone()))
                    .map_err(AppError::Io)?;
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(AppError::Xml(e)),
            Ok(e) => {
                if !inside_tbl {
                    writer.write_event(e).map_err(AppError::Io)?;
                }
            }
        }
    }
    Ok(writer.into_inner())
}

fn text_frame_roundtrip(
    xml_bytes: &[u8],
    shape_idx: usize,
    modifier: impl FnOnce(&mut serde_json::Value, &[path::PathSegment]) -> AppResult<()>,
    remaining: &[path::PathSegment],
) -> AppResult<Vec<u8>> {
    let empty_map = HashMap::new();
    let shapes = crate::model::slide::parse_slide_shapes(xml_bytes, &empty_map)?;

    let shape = shapes
        .get(shape_idx)
        .ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;

    let mut json = serde_json::to_value(shape)
        .map_err(|e| AppError::InvalidValue(format!("Serialization error: {e}")))?;

    modifier(&mut json, remaining)?;

    let modified_shape: ShapeDto = serde_json::from_value(json)
        .map_err(|e| AppError::InvalidValue(format!("Deserialization error: {e}")))?;

    let new_txbody = match modified_shape.text_frame {
        Some(ref tf) => crate::dto::xml::txbody_to_xml(tf),
        None => {
            return Err(AppError::InvalidValue(
                "Target shape has no text frame".to_string(),
            ));
        }
    };

    replace_txbody(xml_bytes, shape_idx, new_txbody.as_bytes())
}

fn table_roundtrip(
    xml_bytes: &[u8],
    shape_idx: usize,
    modifier: impl FnOnce(&mut serde_json::Value, &[path::PathSegment]) -> AppResult<()>,
    remaining: &[path::PathSegment],
) -> AppResult<Vec<u8>> {
    let empty_map = HashMap::new();
    let shapes = crate::model::slide::parse_slide_shapes(xml_bytes, &empty_map)?;

    let shape = shapes
        .get(shape_idx)
        .ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;

    let mut json = serde_json::to_value(shape)
        .map_err(|e| AppError::InvalidValue(format!("Serialization error: {e}")))?;

    modifier(&mut json, remaining)?;

    let modified_shape: ShapeDto = serde_json::from_value(json)
        .map_err(|e| AppError::InvalidValue(format!("Deserialization error: {e}")))?;

    let new_tbl = match modified_shape.table {
        Some(ref tbl) => crate::dto::xml::table_to_xml(tbl),
        None => {
            return Err(AppError::InvalidValue(
                "Target shape has no table".to_string(),
            ));
        }
    };

    replace_table_tbl(xml_bytes, shape_idx, new_tbl.as_bytes())
}

pub fn add_to_table(
    xml_bytes: &[u8],
    shape_idx: usize,
    remaining: &[path::PathSegment],
    value_json: &str,
) -> AppResult<Vec<u8>> {
    let new_val: serde_json::Value = serde_json::from_str(value_json)
        .map_err(|e| AppError::InvalidValue(format!("Invalid JSON: {e}")))?;

    table_roundtrip(
        xml_bytes,
        shape_idx,
        |json, _remaining| {
            let parent = navigate_json_mut(json, &remaining[..remaining.len() - 1])?;
            match remaining.last() {
                Some(path::PathSegment::Index(idx)) => {
                    let arr = parent
                        .as_array_mut()
                        .ok_or_else(|| AppError::PathParse("Expected array".to_string()))?;
                    let insert_at = (*idx + 1).min(arr.len());
                    arr.insert(insert_at, new_val.clone());
                }
                Some(path::PathSegment::Field(name)) => {
                    let arr = parent
                        .get_mut(name.as_str())
                        .and_then(|v| v.as_array_mut())
                        .ok_or_else(|| {
                            AppError::PathParse(format!("Expected array field '{name}'"))
                        })?;
                    arr.push(new_val.clone());
                }
                None => unreachable!(),
            }
            Ok(())
        },
        remaining,
    )
}

pub fn remove_from_table(
    xml_bytes: &[u8],
    shape_idx: usize,
    remaining: &[path::PathSegment],
) -> AppResult<Vec<u8>> {
    table_roundtrip(
        xml_bytes,
        shape_idx,
        |json, _remaining| {
            let parent = navigate_json_mut(json, &remaining[..remaining.len() - 1])?;
            match remaining.last() {
                Some(path::PathSegment::Index(idx)) => {
                    let arr = parent
                        .as_array_mut()
                        .ok_or_else(|| AppError::PathParse("Expected array".to_string()))?;
                    if *idx >= arr.len() {
                        return Err(AppError::PathParse(format!("Index {idx} out of bounds")));
                    }
                    arr.remove(*idx);
                }
                _ => return Err(AppError::PathParse("Expected index to remove".to_string())),
            }
            Ok(())
        },
        remaining,
    )
}

pub fn add_to_text_frame(
    xml_bytes: &[u8],
    shape_idx: usize,
    remaining: &[path::PathSegment],
    value_json: &str,
) -> AppResult<Vec<u8>> {
    let new_val: serde_json::Value = serde_json::from_str(value_json)
        .map_err(|e| AppError::InvalidValue(format!("Invalid JSON: {e}")))?;

    text_frame_roundtrip(
        xml_bytes,
        shape_idx,
        |json, _remaining| {
            // remaining examples:
            //   ["text_frame", "paragraphs", Index(K)] -> insert para at K+1
            //   ["text_frame", "paragraphs"] -> append para
            //   ["text_frame", "paragraphs", Index(K), "runs", Index(J)] -> insert run at J+1
            //   ["text_frame", "paragraphs", Index(K), "runs"] -> append run
            let parent = navigate_json_mut(json, &remaining[..remaining.len() - 1])?;

            match remaining.last() {
                Some(path::PathSegment::Index(idx)) => {
                    let arr = parent
                        .as_array_mut()
                        .ok_or_else(|| AppError::PathParse("Expected array".to_string()))?;
                    let insert_at = (*idx + 1).min(arr.len());
                    arr.insert(insert_at, new_val.clone());
                }
                Some(path::PathSegment::Field(name)) => {
                    let arr = parent
                        .get_mut(name.as_str())
                        .and_then(|v| v.as_array_mut())
                        .ok_or_else(|| {
                            AppError::PathParse(format!("Expected array field '{name}'"))
                        })?;
                    arr.push(new_val.clone());
                }
                None => unreachable!(),
            }
            Ok(())
        },
        remaining,
    )
}

pub fn remove_from_text_frame(
    xml_bytes: &[u8],
    shape_idx: usize,
    remaining: &[path::PathSegment],
) -> AppResult<Vec<u8>> {
    text_frame_roundtrip(
        xml_bytes,
        shape_idx,
        |json, _remaining| {
            // remaining examples:
            //   ["text_frame", "paragraphs", Index(K)] -> remove para K
            //   ["text_frame", "paragraphs", Index(K), "runs", Index(J)] -> remove run J
            let parent = navigate_json_mut(json, &remaining[..remaining.len() - 1])?;
            match remaining.last() {
                Some(path::PathSegment::Index(idx)) => {
                    let arr = parent
                        .as_array_mut()
                        .ok_or_else(|| AppError::PathParse("Expected array".to_string()))?;
                    if *idx >= arr.len() {
                        return Err(AppError::PathParse(format!("Index {idx} out of bounds")));
                    }
                    arr.remove(*idx);
                }
                _ => return Err(AppError::PathParse("Expected index to remove".to_string())),
            }
            Ok(())
        },
        remaining,
    )
}

fn serialize_start_event(ename: &[u8], e: &BytesStart, out: &mut Vec<u8>) {
    out.push(b'<');
    out.extend_from_slice(ename);
    for a in e.attributes().flatten() {
        out.push(b' ');
        out.extend_from_slice(a.key.as_ref());
        out.extend_from_slice(b"=\"");
        out.extend_from_slice(&a.value);
        out.push(b'"');
    }
    out.push(b'>');
}

fn serialize_empty_event(ename: &[u8], e: &BytesStart, out: &mut Vec<u8>) {
    out.push(b'<');
    out.extend_from_slice(ename);
    for a in e.attributes().flatten() {
        out.push(b' ');
        out.extend_from_slice(a.key.as_ref());
        out.extend_from_slice(b"=\"");
        out.extend_from_slice(&a.value);
        out.push(b'"');
    }
    out.extend_from_slice(b"/>");
}

pub fn extract_txbody_element(
    xml_bytes: &[u8],
    shape_idx: usize,
    remaining: &[path::PathSegment],
) -> AppResult<Vec<u8>> {
    let target_para = remaining
        .iter()
        .skip(2) // text_frame, paragraphs
        .find_map(|s| {
            if let path::PathSegment::Index(i) = s {
                Some(*i)
            } else {
                None
            }
        });

    let target_run = remaining
        .iter()
        .skip(4) // text_frame, paragraphs, para_idx, runs
        .find_map(|s| {
            if let path::PathSegment::Index(i) = s {
                Some(*i)
            } else {
                None
            }
        });

    let extracting_run = target_run.is_some();
    let is_run_self_closing = remaining.len() == 5; // ...paragraphs[K].runs[J] and no deeper

    let mut reader = Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true);
    let mut shape_counter = 0;
    let mut inside_target = false;
    let mut in_txbody = false;
    let mut in_para = false;
    let mut collecting = false;
    let mut para_depth = 0usize;
    let mut para_counter = 0usize;
    let mut run_counter = 0usize;
    let mut buf = Vec::new();
    let mut extracted = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let en = e.name();
                let enb = en.as_ref();
                let is_shape = enb == b"p:sp" || enb == b"p:pic" || enb == b"p:graphicFrame";
                if is_shape {
                    if shape_counter == shape_idx {
                        inside_target = true;
                    }
                    shape_counter += 1;
                }
                if inside_target && enb == b"p:txBody" {
                    in_txbody = true;
                }
                if in_txbody && enb == b"a:p" {
                    if para_counter == target_para.unwrap_or(0) {
                        in_para = true;
                        para_depth = 1;
                        if !extracting_run {
                            collecting = true;
                            serialize_start_event(enb, e, &mut extracted);
                        }
                    }
                    para_counter += 1;
                }
                if in_para && enb == b"a:r" && extracting_run {
                    if run_counter == target_run.unwrap_or(0) {
                        collecting = true;
                        if is_run_self_closing {
                            serialize_empty_event(enb, e, &mut extracted);
                            collecting = false;
                        } else {
                            serialize_start_event(enb, e, &mut extracted);
                        }
                    }
                    run_counter += 1;
                }
                if collecting
                    && !(extracting_run && enb == b"a:r")
                    && !(!extracting_run && enb == b"a:p")
                {
                    serialize_start_event(enb, e, &mut extracted);
                }
                if in_para {
                    para_depth += 1;
                }
            }
            Ok(Event::End(ref e)) => {
                let en = e.name();
                let enb = en.as_ref();
                if in_para {
                    para_depth = para_depth.saturating_sub(1);
                    if para_depth == 0 {
                        in_para = false;
                    }
                }
                if in_txbody && enb == b"p:txBody" {
                    in_txbody = false;
                }
                if collecting && (enb == b"a:p" || (extracting_run && enb == b"a:r")) {
                    extracted.extend_from_slice(b"</");
                    extracted.extend_from_slice(enb);
                    extracted.extend_from_slice(b">");
                    collecting = false;
                } else if collecting {
                    extracted.extend_from_slice(b"</");
                    extracted.extend_from_slice(enb);
                    extracted.extend_from_slice(b">");
                }
                if inside_target && (enb == b"p:sp" || enb == b"p:pic" || enb == b"p:graphicFrame")
                {
                    inside_target = false;
                }
            }
            Ok(Event::Empty(ref e)) => {
                let en = e.name();
                if collecting {
                    serialize_empty_event(en.as_ref(), e, &mut extracted);
                }
                if in_para && extracting_run && en.as_ref() == b"a:r" {
                    if run_counter == target_run.unwrap_or(0) {
                        serialize_empty_event(en.as_ref(), e, &mut extracted);
                    }
                    run_counter += 1;
                }
            }
            Ok(Event::Text(ref t)) => {
                if collecting {
                    extracted.extend_from_slice(t);
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(AppError::Xml(e)),
            _ => {}
        }
    }

    if extracted.is_empty() {
        return Err(AppError::PathParse("Element not found".to_string()));
    }
    Ok(extracted)
}

pub fn insert_into_txbody(
    xml_bytes: &[u8],
    shape_idx: usize,
    remaining: &[path::PathSegment],
    element_xml: &[u8],
) -> AppResult<Vec<u8>> {
    let target_para = remaining.iter().skip(2).find_map(|s| {
        if let path::PathSegment::Index(i) = s {
            Some(*i)
        } else {
            None
        }
    });

    let target_run = remaining.iter().skip(4).find_map(|s| {
        if let path::PathSegment::Index(i) = s {
            Some(*i)
        } else {
            None
        }
    });

    let is_run_target = target_run.is_some() || {
        let mut segs = remaining.iter();
        segs.next();
        segs.next();
        matches!(segs.next(), Some(path::PathSegment::Field(n)) if n == "runs")
    };

    let mut reader = Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true);
    let mut writer = Writer::new(Vec::new());
    let mut shape_counter = 0;
    let mut inside_target = false;
    let mut inside_txbody = false;
    let mut in_target_para = false;
    let mut inserted = false;
    let mut para_counter = 0usize;
    let mut run_counter = 0usize;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let en = e.name();
                let enb = en.as_ref();
                let is_shape = enb == b"p:sp" || enb == b"p:pic" || enb == b"p:graphicFrame";
                if is_shape {
                    if shape_counter == shape_idx {
                        inside_target = true;
                    }
                    shape_counter += 1;
                }
                if inside_target && enb == b"p:txBody" {
                    inside_txbody = true;
                }
                if inside_txbody && enb == b"a:p" {
                    para_counter += 1;
                    if Some(para_counter - 1) == target_para
                        || (target_para.is_none() && para_counter == 1)
                    {
                        in_target_para = true;
                    }
                }

                writer
                    .write_event(Event::Start(e.clone()))
                    .map_err(AppError::Io)?;

                if inside_txbody && !inserted {
                    let should_insert = if is_run_target && in_target_para {
                        if let Some(tr) = target_run {
                            enb == b"a:r" && run_counter == tr
                        } else {
                            false
                        }
                    } else if !is_run_target {
                        if let Some(tp) = target_para {
                            enb == b"a:p" && para_counter - 1 == tp
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                    if should_insert {
                        writer
                            .get_mut()
                            .write_all(element_xml)
                            .map_err(AppError::Io)?;
                        inserted = true;
                    }
                }

                if inside_txbody && in_target_para && enb == b"a:r" {
                    run_counter += 1;
                }
            }
            Ok(Event::End(ref e)) => {
                let en = e.name();
                let enb = en.as_ref();

                if inside_txbody
                    && !inserted
                    && ((is_run_target && in_target_para && enb == b"a:p")
                        || (!is_run_target && enb == b"p:txBody"))
                {
                    writer
                        .get_mut()
                        .write_all(element_xml)
                        .map_err(AppError::Io)?;
                    inserted = true;
                }

                writer
                    .write_event(Event::End(e.clone()))
                    .map_err(AppError::Io)?;

                if inside_txbody && enb == b"a:p" {
                    in_target_para = false;
                }
                if inside_txbody && enb == b"p:txBody" {
                    inside_txbody = false;
                }
                if inside_target && (enb == b"p:sp" || enb == b"p:pic" || enb == b"p:graphicFrame")
                {
                    inside_target = false;
                }
            }
            Ok(Event::Empty(ref e)) => {
                writer
                    .write_event(Event::Empty(e.clone()))
                    .map_err(AppError::Io)?;
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(AppError::Xml(e)),
            Ok(e) => {
                writer.write_event(e).map_err(AppError::Io)?;
            }
        }
    }
    Ok(writer.into_inner())
}

fn navigate_json_mut<'a>(
    value: &'a mut serde_json::Value,
    segments: &[path::PathSegment],
) -> AppResult<&'a mut serde_json::Value> {
    let mut current = value;
    for seg in segments {
        current = match seg {
            path::PathSegment::Field(name) => {
                if !current.get(name.as_str()).is_some()
                    && let Some(map) = current.as_object_mut()
                {
                    map.insert(name.clone(), serde_json::Value::Null);
                }
                current.get_mut(name.as_str()).ok_or_else(|| {
                    AppError::PathParse(format!("Field '{name}' not found in JSON tree"))
                })?
            }
            path::PathSegment::Index(idx) => current.get_mut(*idx).ok_or_else(|| {
                AppError::PathParse(format!("Index {idx} out of bounds in JSON tree"))
            })?,
        };
    }
    Ok(current)
}

pub fn find_max_sld_id(xml_bytes: &[u8]) -> u32 {
    let mut reader = Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true);
    let mut max_id = 0u32;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e)) => {
                if e.name().as_ref() == b"p:sldId" {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"id"
                            && let Ok(v) = String::from_utf8_lossy(&attr.value).parse::<u32>()
                            && v > max_id
                        {
                            max_id = v;
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            _ => {}
        }
    }
    max_id
}

pub fn remove_slide_from_presentation(
    xml_bytes: &[u8],
    slide_idx: usize,
) -> AppResult<(Vec<u8>, String)> {
    let mut reader = Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true);
    let mut writer = Writer::new(Vec::new());
    let mut idx = 0usize;
    let mut in_lst = false;
    let mut removed_r_id = String::new();
    let mut skip_depth: Option<usize> = None;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let en = e.name();
                let enb = en.as_ref();
                if enb == b"p:sldIdLst" {
                    in_lst = true;
                }
                if in_lst && enb == b"p:sldId" && idx == slide_idx {
                    skip_depth = Some(1);
                    for a in e.attributes().flatten() {
                        if a.key.as_ref() == b"r:id" {
                            removed_r_id = String::from_utf8_lossy(&a.value).to_string();
                        }
                    }
                    idx += 1;
                    continue;
                }
                if in_lst && enb == b"p:sldId" {
                    idx += 1;
                }
                if let Some(ref mut d) = skip_depth {
                    *d += 1;
                    continue;
                }
                writer
                    .write_event(Event::Start(e.clone()))
                    .map_err(AppError::Io)?;
            }
            Ok(Event::End(ref e)) => {
                if let Some(ref mut d) = skip_depth {
                    *d -= 1;
                    if *d == 0 {
                        skip_depth = None;
                    }
                    continue;
                }
                let en = e.name();
                let enb = en.as_ref();
                if enb == b"p:sldIdLst" {
                    in_lst = false;
                }
                writer
                    .write_event(Event::End(e.clone()))
                    .map_err(AppError::Io)?;
            }
            Ok(Event::Empty(ref e)) => {
                let en = e.name();
                let enb = en.as_ref();
                if in_lst && enb == b"p:sldId" {
                    if idx == slide_idx {
                        for a in e.attributes().flatten() {
                            if a.key.as_ref() == b"r:id" {
                                removed_r_id = String::from_utf8_lossy(&a.value).to_string();
                            }
                        }
                        idx += 1;
                        continue;
                    }
                    idx += 1;
                }
                writer
                    .write_event(Event::Empty(e.clone()))
                    .map_err(AppError::Io)?;
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(AppError::Xml(e)),
            Ok(e) => {
                if skip_depth.is_none() {
                    writer.write_event(e).map_err(AppError::Io)?;
                }
            }
        }
    }

    if removed_r_id.is_empty() {
        return Err(AppError::SlideIndexOutOfBounds(slide_idx));
    }

    Ok((writer.into_inner(), removed_r_id))
}

pub fn insert_slide_into_presentation(
    xml_bytes: &[u8],
    insert_after_idx: usize,
    new_r_id: &str,
    new_sld_id: u32,
) -> AppResult<Vec<u8>> {
    let mut reader = Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true);
    let mut writer = Writer::new(Vec::new());
    let mut idx = 0usize;
    let mut inserted = false;
    let mut in_lst = false;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let en = e.name();
                let enb = en.as_ref();
                if enb == b"p:sldIdLst" {
                    in_lst = true;
                }
                if in_lst && enb == b"p:sldId" {
                    writer
                        .write_event(Event::Start(e.clone()))
                        .map_err(AppError::Io)?;
                    if idx == insert_after_idx && !inserted {
                        let mut new_elem = BytesStart::new("p:sldId");
                        new_elem.push_attribute(("id", new_sld_id.to_string().as_str()));
                        new_elem.push_attribute(("r:id", new_r_id));
                        writer
                            .write_event(Event::Empty(new_elem))
                            .map_err(AppError::Io)?;
                        inserted = true;
                    }
                    idx += 1;
                } else {
                    writer
                        .write_event(Event::Start(e.clone()))
                        .map_err(AppError::Io)?;
                }
            }
            Ok(Event::End(ref e)) => {
                let en = e.name();
                let enb = en.as_ref();
                if enb == b"p:sldIdLst" {
                    if !inserted {
                        let mut new_elem = BytesStart::new("p:sldId");
                        new_elem.push_attribute(("id", new_sld_id.to_string().as_str()));
                        new_elem.push_attribute(("r:id", new_r_id));
                        writer
                            .write_event(Event::Empty(new_elem))
                            .map_err(AppError::Io)?;
                        inserted = true;
                    }
                    in_lst = false;
                }
                writer
                    .write_event(Event::End(e.clone()))
                    .map_err(AppError::Io)?;
            }
            Ok(Event::Empty(ref e)) => {
                let en = e.name();
                let enb = en.as_ref();
                if in_lst && enb == b"p:sldId" {
                    writer
                        .write_event(Event::Empty(e.clone()))
                        .map_err(AppError::Io)?;
                    if idx == insert_after_idx && !inserted {
                        let mut new_elem = BytesStart::new("p:sldId");
                        new_elem.push_attribute(("id", new_sld_id.to_string().as_str()));
                        new_elem.push_attribute(("r:id", new_r_id));
                        writer
                            .write_event(Event::Empty(new_elem))
                            .map_err(AppError::Io)?;
                        inserted = true;
                    }
                    idx += 1;
                } else {
                    writer
                        .write_event(Event::Empty(e.clone()))
                        .map_err(AppError::Io)?;
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(AppError::Xml(e)),
            Ok(e) => {
                writer.write_event(e).map_err(AppError::Io)?;
            }
        }
    }
    Ok(writer.into_inner())
}
