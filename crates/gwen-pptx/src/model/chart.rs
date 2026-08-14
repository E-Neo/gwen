use serde_json::json;

use crate::engine::xml_edit;
use crate::engine::xml_edit::find_elem_range;

/// Read all `c:v` values under a cache (`c:strCache`/`c:numCache`/`c:strLit`/`c:numLit`)
/// located within `[start, end]`.
fn read_cache_values(
    events: &[quick_xml::events::Event<'_>],
    start: usize,
    end: usize,
) -> Vec<String> {
    let cache_names: [&[u8]; 4] = [b"c:strCache", b"c:numCache", b"c:strLit", b"c:numLit"];
    let Some((cache_start, cache_end)) = cache_names
        .iter()
        .find_map(|n| find_elem_range(events, n, start).filter(|r| r.0 <= end))
    else {
        return Vec::new();
    };

    let mut out = Vec::new();
    let mut i = cache_start + 1;
    while i < cache_end {
        match &events[i] {
            quick_xml::events::Event::Start(e) if e.name().as_ref() == b"c:pt" => {
                if let Some((vs, ve)) =
                    find_elem_range(events, b"c:v", i).filter(|r| r.0 <= cache_end)
                {
                    let text = (vs + 1..ve)
                        .filter_map(|j| match &events[j] {
                            quick_xml::events::Event::Text(t) => Some(t),
                            _ => None,
                        })
                        .map(|t| String::from_utf8_lossy(t.as_ref()).to_string())
                        .collect::<String>();
                    out.push(text);
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

fn read_series_name(
    events: &[quick_xml::events::Event<'_>],
    ser_start: usize,
    ser_end: usize,
) -> Option<String> {
    let tx_range = find_elem_range(events, b"c:tx", ser_start).filter(|r| r.0 <= ser_end)?;
    let values = read_cache_values(events, tx_range.0, tx_range.1);
    values.into_iter().next()
}

/// Query a chart part: `{ "chart_type": ..., "series": [ { "name", "categories", "values" }, ... ] }`.
pub fn parse_chart(xml: &[u8]) -> serde_json::Value {
    let events = match xml_edit::read_events(xml) {
        Ok(e) => e,
        Err(_) => return json!({}),
    };

    let Some((chart_start, chart_end)) = find_elem_range(&events, b"c:chart", 0) else {
        return json!({});
    };
    let Some((plot_start, plot_end)) =
        find_elem_range(&events, b"c:plotArea", chart_start).filter(|r| r.0 <= chart_end)
    else {
        return json!({});
    };

    // Chart type is the first child of plotArea that is a chart-type element.
    let chart_type = {
        let mut name = None;
        let mut i = plot_start + 1;
        while i < plot_end {
            if let quick_xml::events::Event::Start(e) = &events[i]
                && xml_edit::is_chart_type_tag(e.name().as_ref())
            {
                name = Some(String::from_utf8_lossy(e.name().as_ref()).to_string());
                break;
            }
            i += 1;
        }
        name
    };

    // Collect every c:ser element directly under the chart-type element.
    let mut series = Vec::new();
    let mut i = plot_start + 1;
    while i < plot_end {
        if let quick_xml::events::Event::Start(e) = &events[i]
            && e.name().as_ref() == b"c:ser"
        {
            let (ser_start, ser_end) = find_elem_range(&events, b"c:ser", i).unwrap();
            let name = read_series_name(&events, ser_start, ser_end).unwrap_or_default();
            let categories = find_elem_range(&events, b"c:cat", ser_start)
                .filter(|r| r.0 <= ser_end)
                .map(|(s, e)| read_cache_values(&events, s, e))
                .unwrap_or_default();
            let values = find_elem_range(&events, b"c:val", ser_start)
                .filter(|r| r.0 <= ser_end)
                .map(|(s, e)| {
                    read_cache_values(&events, s, e)
                        .into_iter()
                        .map(|v| {
                            v.parse::<f64>()
                                .map(|n| json!(n))
                                .unwrap_or_else(|_| json!(v))
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            series.push(json!({
                "name": name,
                "categories": categories,
                "values": values,
            }));
            i = ser_end;
        }
        i += 1;
    }

    json!({ "chart_type": chart_type, "series": series })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHART: &str = r#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart>
        <c:ser>
          <c:tx><c:strRef><c:strCache><c:ptCount val="1"/><c:pt idx="0"><c:v>Series 1</c:v></c:pt></c:strCache></c:strRef></c:tx>
          <c:cat><c:strRef><c:strCache><c:ptCount val="2"/><c:pt idx="0"><c:v>North</c:v></c:pt><c:pt idx="1"><c:v>South</c:v></c:pt></c:strCache></c:strRef></c:cat>
          <c:val><c:numRef><c:numCache><c:ptCount val="2"/><c:pt idx="0"><c:v>20</c:v></c:pt><c:pt idx="1"><c:v>50</c:v></c:pt></c:numCache></c:numRef></c:val>
        </c:ser>
      </c:barChart></c:plotArea></c:chart></c:chartSpace>"#;

    #[test]
    fn parses_chart_type_and_series() {
        let v = parse_chart(CHART.as_bytes());
        assert_eq!(v["chart_type"], "c:barChart");
        assert_eq!(v["series"][0]["name"], "Series 1");
        assert_eq!(v["series"][0]["categories"], json!(["North", "South"]));
        assert_eq!(v["series"][0]["values"], json!([20.0, 50.0]));
    }

    #[test]
    fn handles_empty_chart() {
        let v = parse_chart(b"<c:chartSpace xmlns:c=\"x\"/>");
        assert!(v.get("series").is_none() || v["series"].as_array().is_some());
    }
}
