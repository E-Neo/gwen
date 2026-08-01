use std::collections::HashMap;

use crate::error::AppResult;

pub fn sanitize_subtree(
    subtree_xml: &[u8],
    new_id: u32,
    r_id_map: &HashMap<String, String>,
) -> AppResult<Vec<u8>> {
    let text = std::str::from_utf8(subtree_xml)
        .map_err(|e| crate::error::AppError::InvalidValue(format!("Invalid UTF-8: {}", e)))?;

    let mut result = text.to_string();

    result = regex_replace_once(&result, r#"id="\d+""#, &format!(r#"id="{}""#, new_id));

    for (old_r_id, new_r_id) in r_id_map {
        result = result.replace(
            &format!(r#"r:embed="{}""#, old_r_id),
            &format!(r#"r:embed="{}""#, new_r_id),
        );
        result = result.replace(
            &format!(r#"r:id="{}""#, old_r_id),
            &format!(r#"r:id="{}""#, new_r_id),
        );
    }

    Ok(result.into_bytes())
}

/// Byte-level replacement of relationship attribute values (`r:embed`/`r:id`).
pub fn replace_r_ids(xml: &[u8], r_id_map: &HashMap<String, String>) -> Vec<u8> {
    let mut result = xml.to_vec();
    for (old_r_id, new_r_id) in r_id_map {
        for attr in ["r:embed", "r:id"] {
            let old_bytes = format!("{attr}=\"{old_r_id}\"").into_bytes();
            let new_bytes = format!("{attr}=\"{new_r_id}\"").into_bytes();
            result = replace_bytes(&result, &old_bytes, &new_bytes);
        }
    }
    result
}

fn replace_bytes(data: &[u8], old: &[u8], new: &[u8]) -> Vec<u8> {
    if old.is_empty() {
        return data.to_vec();
    }
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if i + old.len() <= data.len() && data[i..i + old.len()] == *old {
            out.extend_from_slice(new);
            i += old.len();
        } else {
            out.push(data[i]);
            i += 1;
        }
    }
    out
}

fn regex_replace_once(text: &str, pattern: &str, replacement: &str) -> String {
    if let Some(start) = text.find(pattern) {
        let mut result = text.to_string();
        result.replace_range(start..start + pattern.len(), replacement);
        result
    } else {
        text.to_string()
    }
}
