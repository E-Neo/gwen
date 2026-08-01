use std::collections::HashMap;

use crate::error::AppResult;

pub fn sanitize_subtree(
    subtree_xml: &[u8],
    new_id: u32,
    r_id_map: &HashMap<String, String>,
) -> AppResult<Vec<u8>> {
    let text = std::str::from_utf8(subtree_xml)
        .map_err(|e| crate::error::AppError::InvalidValue(format!("Invalid UTF-8: {}", e)))?;

    // Remap every shape id (cNvPr id="N") in the copied subtree to a fresh,
    // incrementing id so nested shapes (e.g. inside a copied group) don't
    // collide with ids already present at the destination.
    let mut result = String::new();
    let mut rest = text;
    let mut next_id = new_id;
    loop {
        match rest.find("id=\"") {
            Some(offset) => {
                result.push_str(&rest[..offset]);
                let after = &rest[offset + 4..];
                let value_end = after.find('"').ok_or_else(|| {
                    crate::error::AppError::InvalidValue(
                        "unterminated id attribute in copied subtree".to_string(),
                    )
                })?;
                let value = &after[..value_end];
                if value.chars().all(|c| c.is_ascii_digit()) {
                    result.push_str(&format!("id=\"{}\"", next_id));
                    next_id += 1;
                } else {
                    // Non-numeric id (e.g. r:id="rId2"); leave as-is. r:embed /
                    // r:id values are remapped separately by the caller.
                    result.push_str(&rest[..offset + 4 + value_end + 1]);
                }
                rest = &after[value_end + 1..];
            }
            None => {
                result.push_str(rest);
                break;
            }
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remaps_all_shape_ids_in_subtree() {
        let subtree = br#"<p:grpSp>
            <p:nvGrpSpPr><p:cNvPr id="2" name="G"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>
            <p:grpSpPr/>
            <p:sp><p:nvSpPr><p:cNvPr id="3" name="A"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr/></p:sp>
            <p:sp><p:nvSpPr><p:cNvPr id="4" name="B"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr/></p:sp>
        </p:grpSp>"#;
        let out =
            String::from_utf8(sanitize_subtree(subtree, 10, &HashMap::new()).unwrap()).unwrap();
        assert!(out.contains(r#"p:cNvPr id="10" name="G""#), "got: {out}");
        assert!(out.contains(r#"p:cNvPr id="11" name="A""#), "got: {out}");
        assert!(out.contains(r#"p:cNvPr id="12" name="B""#), "got: {out}");
        assert!(!out.contains(r#"p:cNvPr id="2""#));
        assert!(!out.contains(r#"p:cNvPr id="3""#));
        assert!(!out.contains(r#"p:cNvPr id="4""#));
    }

    #[test]
    fn leaves_non_numeric_ids_and_r_ids_alone() {
        let subtree =
            br#"<p:sp><p:nvSpPr><p:cNvPr id="2" name="X"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>
            <p:spPr><a:blip r:embed="rId5"/></p:spPr></p:sp>"#;
        let mut r_id_map = HashMap::new();
        r_id_map.insert("rId5".to_string(), "rId99".to_string());
        let out = String::from_utf8(sanitize_subtree(subtree, 10, &r_id_map).unwrap()).unwrap();
        assert!(out.contains(r#"p:cNvPr id="10""#), "got: {out}");
        assert!(out.contains(r#"r:embed="rId99""#), "got: {out}");
    }
}
