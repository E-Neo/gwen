use std::collections::HashMap;
use std::io::Write;

use quick_xml::Reader;
use quick_xml::Writer;
use quick_xml::events::{BytesStart, Event};

use crate::dto::ShapeDto;
use crate::error::{AppError, AppResult};
use crate::path;

/// True for any element that opens a shape: a regular shape, picture,
/// connector, group, or graphic frame (table/chart).
fn is_shape_tag(name: &[u8]) -> bool {
    matches!(
        name,
        b"p:sp" | b"p:pic" | b"p:cxnSp" | b"p:grpSp" | b"p:graphicFrame"
    )
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
                let is_shape = is_shape_tag(e.name().as_ref());
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
    let mut insert_after_depth: Option<usize> = None;
    let mut buffer = Vec::new();

    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(ref e)) => {
                writer
                    .write_event(Event::Start(e.clone()))
                    .map_err(AppError::Io)?;
                let is_shape = is_shape_tag(e.name().as_ref());
                if is_shape {
                    if shape_counter == insert_idx {
                        insert_after_depth = Some(0);
                    }
                    shape_counter += 1;
                }
                if let Some(ref mut d) = insert_after_depth {
                    *d += 1;
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
                if let Some(ref mut d) = insert_after_depth {
                    *d -= 1;
                    if *d == 0 {
                        writer
                            .get_mut()
                            .write_all(new_shape_xml)
                            .map_err(AppError::Io)?;
                        inserted = true;
                        insert_after_depth = None;
                    }
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
                let is_shape = is_shape_tag(e.name().as_ref());
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

                if inside_target && (is_shape_tag(e.name().as_ref())) {
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
                let is_shape = is_shape_tag(ename_bytes);
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
                if inside_target && (is_shape_tag(e.name().as_ref())) {
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
                let is_shape = is_shape_tag(enb);
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
                if inside_target && (is_shape_tag(enb)) {
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
