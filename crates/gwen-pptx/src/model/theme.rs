use serde_json::json;

use crate::xml_parse::{THEME_COLOR_NAMES, find_child_elem_range, find_elem_range, read_events};

fn read_attr(events: &[quick_xml::events::Event<'_>], i: usize, key: &[u8]) -> Option<String> {
    let e = match &events[i] {
        quick_xml::events::Event::Empty(e) | quick_xml::events::Event::Start(e) => e,
        _ => return None,
    };
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == key)
        .map(|a| String::from_utf8_lossy(&a.value).to_string())
}

/// Query a theme part: `{ "colors": {...}, "fonts": { "major": ..., "minor": ... } }`.
pub fn parse_theme(xml: &[u8]) -> serde_json::Value {
    let events = match read_events(xml) {
        Ok(e) => e,
        Err(_) => return json!({}),
    };

    let mut colors = serde_json::Map::new();
    if let Some((cs, ce)) = find_elem_range(&events, b"a:clrScheme", 0) {
        for name in THEME_COLOR_NAMES {
            let child_name = format!("a:{name}");
            let value = find_child_elem_range(&events, cs, ce, child_name.as_bytes())
                .and_then(|(s, e)| {
                    find_child_elem_range(&events, s, e, b"a:srgbClr").and_then(|(s, e)| {
                        if s == e {
                            read_attr(&events, s, b"val")
                        } else {
                            None
                        }
                    })
                })
                .unwrap_or_default();
            colors.insert(name.to_string(), json!(value));
        }
    }

    let mut fonts = serde_json::Map::new();
    if let Some((fs, fe)) = find_elem_range(&events, b"a:fontScheme", 0) {
        for (key, family) in [("major", b"a:majorFont"), ("minor", b"a:minorFont")] {
            let value = find_child_elem_range(&events, fs, fe, family)
                .and_then(|(s, e)| {
                    find_child_elem_range(&events, s, e, b"a:latin").and_then(|(s, e)| {
                        if s == e {
                            read_attr(&events, s, b"typeface")
                        } else {
                            None
                        }
                    })
                })
                .unwrap_or_default();
            fonts.insert(key.to_string(), json!(value));
        }
    }

    json!({ "colors": colors, "fonts": fonts })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_theme_colors_and_fonts() {
        let xml = br#"<a:theme xmlns:a="x"><a:themeElements>
            <a:clrScheme name="Office">
                <a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1>
                <a:accent1><a:srgbClr val="4F81BD"/></a:accent1>
            </a:clrScheme>
            <a:fontScheme name="Office">
                <a:majorFont><a:latin typeface="Calibri Light"/></a:majorFont>
                <a:minorFont><a:latin typeface="Calibri"/></a:minorFont>
            </a:fontScheme>
        </a:themeElements></a:theme>"#;
        let v = parse_theme(xml);
        assert_eq!(v["colors"]["accent1"], "4F81BD");
        assert_eq!(v["fonts"]["major"], "Calibri Light");
        assert_eq!(v["fonts"]["minor"], "Calibri");
    }

    #[test]
    fn dk1_sysclr_reads_empty_when_no_lastclr() {
        let xml = br#"<a:theme xmlns:a="x"><a:themeElements><a:clrScheme name="Office"><a:dk1><a:sysClr val="windowText"/></a:dk1></a:clrScheme></a:themeElements></a:theme>"#;
        let v = parse_theme(xml);
        assert_eq!(v["colors"]["dk1"], "");
    }
}
