use std::collections::HashMap;
use std::io::Write;

use quick_xml::Reader;
use quick_xml::Writer;
use quick_xml::events::{BytesText, Event};

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
                if e.name().as_ref() == b"p:sp" || e.name().as_ref() == b"p:pic" {
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
                let is_shape = e.name().as_ref() == b"p:sp" || e.name().as_ref() == b"p:pic";
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
                let is_shape = e.name().as_ref() == b"p:sp" || e.name().as_ref() == b"p:pic";
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
                let is_shape = e.name().as_ref() == b"p:sp" || e.name().as_ref() == b"p:pic";
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
                if is_sp_tree && !inserted && shape_counter == 0 {
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

#[allow(dead_code)]
pub fn append_shape(xml_bytes: &[u8], new_shape_xml: &[u8]) -> AppResult<Vec<u8>> {
    let mut reader = Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true);
    let mut writer = Writer::new(Vec::new());
    let mut inserted = false;
    let mut buffer = Vec::new();

    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::End(ref e)) => {
                writer
                    .write_event(Event::End(e.clone()))
                    .map_err(AppError::Io)?;
                if e.name().as_ref() == b"p:spTree" && !inserted {
                    writer
                        .get_mut()
                        .write_all(new_shape_xml)
                        .map_err(AppError::Io)?;
                    inserted = true;
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
                let is_shape = e.name().as_ref() == b"p:sp" || e.name().as_ref() == b"p:pic";
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

                if inside_target && (e.name().as_ref() == b"p:sp" || e.name().as_ref() == b"p:pic")
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
