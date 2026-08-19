use quick_xml::Reader;
use quick_xml::events::Event;

use crate::error::{AppError, AppResult};
use crate::xml_parse::core_prop_key;

/// Parse the standard core properties from a `docProps/core.xml` part into a
/// JSON object keyed by snake_case property name (e.g. `title`, `author`).
pub fn parse_core_properties(data: &[u8]) -> AppResult<serde_json::Value> {
    let mut reader = Reader::from_reader(data);
    let mut result = serde_json::Map::new();
    let mut current_tag: Option<String> = None;
    let mut current_text = String::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                current_tag = Some(String::from_utf8_lossy(e.name().as_ref()).to_string());
                current_text.clear();
            }
            Ok(Event::Text(t)) => {
                let s = String::from_utf8_lossy(t.as_ref()).into_owned();
                if !s.trim().is_empty() {
                    current_text.push_str(s.trim());
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if let Some(tag) = &current_tag
                    && *tag == name
                    && let Some(key) = core_prop_key(&name)
                {
                    result.insert(
                        key.to_string(),
                        serde_json::Value::String(current_text.clone()),
                    );
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(AppError::Xml(e)),
            _ => {}
        }
    }

    Ok(serde_json::Value::Object(result))
}

#[cfg(test)]
mod tests {
    use super::parse_core_properties;

    #[test]
    fn parses_present_and_empty_properties() {
        let xml = r#"<?xml version='1.0'?><cp:coreProperties xmlns:cp="x" xmlns:dc="y" xmlns:dcterms="z"><dc:title>My Deck</dc:title><dc:subject/><dc:creator>alice</dc:creator><cp:revision>3</cp:revision><dcterms:created>2020-01-01T00:00:00Z</dcterms:created></cp:coreProperties>"#;
        let v = parse_core_properties(xml.as_bytes()).unwrap();
        assert_eq!(v["title"], "My Deck");
        assert_eq!(v["author"], "alice");
        assert_eq!(v["revision"], "3");
        assert_eq!(v["created"], "2020-01-01T00:00:00Z");
        assert_eq!(v["comments"], serde_json::Value::Null);
    }
}
