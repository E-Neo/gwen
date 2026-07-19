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

fn regex_replace_once(text: &str, pattern: &str, replacement: &str) -> String {
    if let Some(start) = text.find(pattern) {
        let mut result = text.to_string();
        result.replace_range(start..start + pattern.len(), replacement);
        result
    } else {
        text.to_string()
    }
}
