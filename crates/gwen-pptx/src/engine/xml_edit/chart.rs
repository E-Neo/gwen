use super::*;

fn replace_pt_text(
    events: &mut Vec<Event<'static>>,
    container_start: usize,
    container_end: usize,
    pt_idx: usize,
    value: &str,
) -> AppResult<()> {
    let cache_names: [&[u8]; 4] = [b"c:strCache", b"c:numCache", b"c:strLit", b"c:numLit"];
    let cache_range = cache_names
        .iter()
        .find_map(|n| find_elem_range(events, n, container_start).filter(|r| r.0 <= container_end))
        .ok_or_else(|| AppError::PathParse("Chart cache not found".to_string()))?;

    let (pt_start, pt_end) =
        find_nth_child_range(events, cache_range.0, cache_range.1, b"c:pt", pt_idx)
            .ok_or_else(|| AppError::PathParse(format!("Chart point {pt_idx} not found")))?;

    let (v_start, v_end) = find_elem_range(events, b"c:v", pt_start)
        .filter(|r| r.0 <= pt_end)
        .ok_or_else(|| AppError::PathParse("Chart value element not found".to_string()))?;

    replace_text_in_range(events, v_start, v_end, value);
    Ok(())
}

pub fn replace_chart_property_lossless(
    xml_bytes: &[u8],
    remaining: &[path::PathSegment],
    value: &str,
) -> AppResult<Vec<u8>> {
    let inner = if matches!(&remaining[0], path::PathSegment::Field(n) if n == "chart") {
        &remaining[1..]
    } else {
        remaining
    };
    if inner.is_empty() {
        return Err(AppError::PathParse("Empty chart path".to_string()));
    }

    let mut events = read_events(xml_bytes)?;

    let chart_range = find_elem_range(&events, b"c:chart", 0)
        .ok_or_else(|| AppError::PathParse("Chart element not found".to_string()))?;
    let plot_range = find_elem_range(&events, b"c:plotArea", chart_range.0)
        .filter(|r| r.0 <= chart_range.1)
        .ok_or_else(|| AppError::PathParse("Chart plotArea not found".to_string()))?;

    match &inner[0] {
        path::PathSegment::Field(name) if name == "chart_type" => {
            let ct_start = events[plot_range.0..=plot_range.1]
                .iter()
                .enumerate()
                .find_map(|(i, ev)| match ev {
                    Event::Start(e) if is_chart_type_tag(e.name().as_ref()) => {
                        Some(plot_range.0 + i)
                    }
                    _ => None,
                })
                .ok_or_else(|| AppError::PathParse("Chart type element not found".to_string()))?;
            let ct_name = match &events[ct_start] {
                Event::Start(e) => e.name().as_ref().to_vec(),
                _ => unreachable!(),
            };
            let (ct_start, ct_end) = find_elem_range(&events, &ct_name, ct_start).unwrap();

            let new_name = if value.starts_with("c:") {
                value.to_string()
            } else {
                format!("c:{value}")
            };
            events[ct_start] = Event::Start(BytesStart::new(new_name.clone()));
            events[ct_end] = Event::End(BytesEnd::new(new_name));
        }
        path::PathSegment::Field(name) if name == "series" => {
            let ser_idx = match inner.get(1) {
                Some(path::PathSegment::Index(i)) => *i,
                _ => return Err(AppError::PathParse("Expected series index".to_string())),
            };
            let prop = match inner.get(2) {
                Some(path::PathSegment::Field(n)) => n.as_str(),
                _ => {
                    return Err(AppError::PathParse(
                        "Expected series property (name/categories/values)".to_string(),
                    ));
                }
            };
            let pt_idx = match inner.get(3) {
                Some(path::PathSegment::Index(i)) => Some(*i),
                _ => None,
            };

            let (ser_start, ser_end) =
                find_nth_elem_range(&events, b"c:ser", plot_range.0, plot_range.1, ser_idx)
                    .ok_or_else(|| {
                        AppError::PathParse(format!("Series index {ser_idx} not found"))
                    })?;

            match prop {
                "name" => {
                    if pt_idx.is_some() {
                        return Err(AppError::PathParse(
                            "Series name takes no index".to_string(),
                        ));
                    }
                    let tx_range = find_elem_range(&events, b"c:tx", ser_start)
                        .filter(|r| r.0 <= ser_end)
                        .ok_or_else(|| AppError::PathParse("Series has no name".to_string()))?;
                    replace_pt_text(&mut events, tx_range.0, tx_range.1, 0, value)?;
                }
                "categories" | "values" => {
                    let pt_idx = pt_idx.ok_or_else(|| {
                        AppError::PathParse(format!("{prop} point index required"))
                    })?;
                    let target_name: &[u8] = if prop == "categories" {
                        b"c:cat"
                    } else {
                        b"c:val"
                    };
                    let target_range = find_elem_range(&events, target_name, ser_start)
                        .filter(|r| r.0 <= ser_end)
                        .ok_or_else(|| AppError::PathParse(format!("Series has no {prop}")))?;
                    replace_pt_text(&mut events, target_range.0, target_range.1, pt_idx, value)?;
                }
                _ => {
                    return Err(AppError::PathParse(format!(
                        "Unknown series property '{prop}'"
                    )));
                }
            }
        }
        _ => {
            return Err(AppError::PathParse(format!(
                "Unknown chart property '{}'",
                match &inner[0] {
                    path::PathSegment::Field(n) => n.as_str(),
                    _ => "?",
                }
            )));
        }
    }

    write_events(events)
}

/// Build the XML events for a new `<c:ser>` element from a chart series DTO.
fn chart_series_events(series: &crate::dto::ChartSeriesDto, idx: usize) -> Vec<Event<'static>> {
    let mut out = Vec::new();
    out.push(Event::Start(BytesStart::new("c:ser")));
    let mut idx_el = BytesStart::new("c:idx");
    idx_el.push_attribute(("val", idx.to_string().as_str()));
    out.push(Event::Empty(idx_el));
    let mut order_el = BytesStart::new("c:order");
    order_el.push_attribute(("val", idx.to_string().as_str()));
    out.push(Event::Empty(order_el));

    if let Some(name) = &series.name {
        out.push(Event::Start(BytesStart::new("c:tx")));
        out.push(Event::Start(BytesStart::new("c:strRef")));
        out.push(Event::Start(BytesStart::new("c:strCache")));
        let mut pt_count = BytesStart::new("c:ptCount");
        pt_count.push_attribute(("val", "1"));
        out.push(Event::Empty(pt_count));
        out.push(Event::Start(BytesStart::new("c:pt")));
        out.push(Event::Start(BytesStart::new("c:v")));
        out.push(Event::Text(BytesText::from_escaped(escape(name.clone()))));
        out.push(Event::End(BytesEnd::new("c:v")));
        out.push(Event::End(BytesEnd::new("c:pt")));
        out.push(Event::End(BytesEnd::new("c:strCache")));
        out.push(Event::End(BytesEnd::new("c:strRef")));
        out.push(Event::End(BytesEnd::new("c:tx")));
    }

    out.push(Event::Start(BytesStart::new("c:cat")));
    out.push(Event::Start(BytesStart::new("c:strRef")));
    out.push(Event::Start(BytesStart::new("c:strCache")));
    let mut pt_count = BytesStart::new("c:ptCount");
    pt_count.push_attribute(("val", series.categories.len().to_string().as_str()));
    out.push(Event::Empty(pt_count));
    for (j, cat) in series.categories.iter().enumerate() {
        let mut pt = BytesStart::new("c:pt");
        pt.push_attribute(("idx", j.to_string().as_str()));
        out.push(Event::Start(pt));
        out.push(Event::Start(BytesStart::new("c:v")));
        out.push(Event::Text(BytesText::from_escaped(escape(cat.clone()))));
        out.push(Event::End(BytesEnd::new("c:v")));
        out.push(Event::End(BytesEnd::new("c:pt")));
    }
    out.push(Event::End(BytesEnd::new("c:strCache")));
    out.push(Event::End(BytesEnd::new("c:strRef")));
    out.push(Event::End(BytesEnd::new("c:cat")));

    out.push(Event::Start(BytesStart::new("c:val")));
    out.push(Event::Start(BytesStart::new("c:numRef")));
    out.push(Event::Start(BytesStart::new("c:numCache")));
    let format = BytesStart::new("c:formatCode");
    out.push(Event::Start(format));
    out.push(Event::Text(BytesText::from_escaped(escape(
        "General".to_string(),
    ))));
    out.push(Event::End(BytesEnd::new("c:formatCode")));
    let mut pt_count = BytesStart::new("c:ptCount");
    pt_count.push_attribute(("val", series.values.len().to_string().as_str()));
    out.push(Event::Empty(pt_count));
    for (j, val) in series.values.iter().enumerate() {
        let mut pt = BytesStart::new("c:pt");
        pt.push_attribute(("idx", j.to_string().as_str()));
        out.push(Event::Start(pt));
        out.push(Event::Start(BytesStart::new("c:v")));
        out.push(Event::Text(BytesText::from_escaped(escape(
            val.to_string(),
        ))));
        out.push(Event::End(BytesEnd::new("c:v")));
        out.push(Event::End(BytesEnd::new("c:pt")));
    }
    out.push(Event::End(BytesEnd::new("c:numCache")));
    out.push(Event::End(BytesEnd::new("c:numRef")));
    out.push(Event::End(BytesEnd::new("c:val")));

    out.push(Event::End(BytesEnd::new("c:ser")));
    out
}

/// Add a chart series (`chart.series` appends, `chart.series[N]` inserts after N).
pub fn add_chart_series_lossless(
    xml_bytes: &[u8],
    remaining: &[path::PathSegment],
    value_json: &str,
) -> AppResult<Vec<u8>> {
    let inner = if matches!(&remaining[0], path::PathSegment::Field(n) if n == "chart") {
        &remaining[1..]
    } else {
        remaining
    };
    if !matches!(inner, [path::PathSegment::Field(n), ..] if n == "series") {
        return Err(AppError::PathParse(
            "Expected chart.series or chart.series[N]".to_string(),
        ));
    }
    let insert_after = match inner.get(1) {
        Some(path::PathSegment::Index(i)) => Some(*i),
        Some(path::PathSegment::Field(_)) | None => None,
    };

    let series: crate::dto::ChartSeriesDto = serde_json::from_str(value_json)
        .map_err(|e| AppError::InvalidValue(format!("Invalid series JSON: {e}")))?;

    let mut events = read_events(xml_bytes)?;
    let (ct_start, ct_end) = find_chart_type_range(&events)
        .ok_or_else(|| AppError::PathParse("Chart type element not found".to_string()))?;
    let next_idx = count_series(&events, ct_start, ct_end);
    let new_events = chart_series_events(&series, next_idx);

    // Insert position: after the (insert_after)th c:ser end, else before ct_end.
    let insert_pos = if let Some(after) = insert_after {
        let mut count = 0usize;
        let mut pos = None;
        let mut i = ct_start + 1;
        while i < ct_end {
            if let Event::Start(e) = &events[i]
                && e.name().as_ref() == b"c:ser"
            {
                if count == after {
                    let (_, e) = find_elem_range(&events, b"c:ser", i).unwrap();
                    pos = Some(e + 1);
                    break;
                }
                count += 1;
            }
            i += 1;
        }
        pos.unwrap_or(ct_end)
    } else {
        ct_end
    };

    for ev in new_events.into_iter().rev() {
        events.insert(insert_pos, ev);
    }
    write_events(events)
}

/// Remove a chart series (`chart.series[N]`).
pub fn remove_chart_series_lossless(
    xml_bytes: &[u8],
    remaining: &[path::PathSegment],
) -> AppResult<Vec<u8>> {
    let inner = if matches!(&remaining[0], path::PathSegment::Field(n) if n == "chart") {
        &remaining[1..]
    } else {
        remaining
    };
    let ser_idx = match inner {
        [path::PathSegment::Field(n), path::PathSegment::Index(i)] if n == "series" => *i,
        _ => {
            return Err(AppError::PathParse("Expected chart.series[N]".to_string()));
        }
    };

    let mut events = read_events(xml_bytes)?;
    let (ct_start, ct_end) = find_chart_type_range(&events)
        .ok_or_else(|| AppError::PathParse("Chart type element not found".to_string()))?;
    let (ser_start, ser_end) = find_nth_elem_range(&events, b"c:ser", ct_start, ct_end, ser_idx)
        .ok_or_else(|| AppError::PathParse(format!("Series {ser_idx} not found")))?;
    for j in (ser_start..=ser_end).rev() {
        events.remove(j);
    }
    write_events(events)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Replace a single chart series (`c:ser`) with a freshly serialized
/// `ChartSeriesDto`. `remaining` is `chart.series[N]`.
pub fn replace_chart_series_lossless(
    xml_bytes: &[u8],
    remaining: &[path::PathSegment],
    value: &str,
) -> AppResult<Vec<u8>> {
    let inner = if matches!(&remaining[0], path::PathSegment::Field(n) if n == "chart") {
        &remaining[1..]
    } else {
        remaining
    };
    let ser_idx = match inner {
        [path::PathSegment::Field(n), path::PathSegment::Index(i)] if n == "series" => *i,
        _ => {
            return Err(AppError::PathParse("Expected chart.series[N]".to_string()));
        }
    };
    let series: crate::dto::ChartSeriesDto = serde_json::from_str(value)
        .map_err(|e| AppError::InvalidValue(format!("Invalid chart series JSON: {e}")))?;
    let inner_events =
        read_events(crate::dto::xml::chart_series_to_xml(&series, ser_idx).as_bytes())?;

    let mut events = read_events(xml_bytes)?;
    let (ct_start, ct_end) = find_chart_type_range(&events)
        .ok_or_else(|| AppError::PathParse("Chart type element not found".to_string()))?;
    let (ser_start, ser_end) = find_nth_elem_range(&events, b"c:ser", ct_start, ct_end, ser_idx)
        .ok_or_else(|| AppError::PathParse(format!("Series {ser_idx} not found")))?;
    events.splice(ser_start..=ser_end, inner_events);
    write_events(events)
}

/// Append a chart point (`c:pt`) to a series' categories or values cache,
/// bumping the `ptCount`. `remaining` is `chart.series[N].categories` or
/// `chart.series[N].values`; the index comes from `pt_idx` being one past the
/// current end. `value` is the new category/value.
pub fn add_chart_point_lossless(
    xml_bytes: &[u8],
    remaining: &[path::PathSegment],
    pt_idx: usize,
    value: &str,
) -> AppResult<Vec<u8>> {
    let inner = if matches!(&remaining[0], path::PathSegment::Field(n) if n == "chart") {
        &remaining[1..]
    } else {
        remaining
    };
    let (ser_idx, prop) = match inner {
        [
            path::PathSegment::Field(n),
            path::PathSegment::Index(i),
            path::PathSegment::Field(p),
        ] if n == "series" && (p == "categories" || p == "values") => (*i, p.as_str()),
        _ => {
            return Err(AppError::PathParse(
                "Expected chart.series[N].categories or .values".to_string(),
            ));
        }
    };

    let mut events = read_events(xml_bytes)?;
    let (ct_start, ct_end) = find_chart_type_range(&events)
        .ok_or_else(|| AppError::PathParse("Chart type element not found".to_string()))?;
    let (ser_start, ser_end) = find_nth_elem_range(&events, b"c:ser", ct_start, ct_end, ser_idx)
        .ok_or_else(|| AppError::PathParse(format!("Series {ser_idx} not found")))?;

    let (target_name, cache_names): (&[u8], [&[u8]; 2]) = if prop == "categories" {
        (b"c:cat", [b"c:strCache", b"c:strLit"])
    } else {
        (b"c:val", [b"c:numCache", b"c:numLit"])
    };
    let target_range = find_elem_range(&events, target_name, ser_start)
        .filter(|r| r.0 <= ser_end)
        .ok_or_else(|| AppError::PathParse(format!("Series has no {prop}")))?;
    let cache_range = cache_names
        .iter()
        .find_map(|n| find_elem_range(&events, n, target_range.0).filter(|r| r.0 <= target_range.1))
        .ok_or_else(|| AppError::PathParse(format!("Series {prop} cache not found")))?;

    let mut pt = BytesStart::new("c:pt");
    pt.push_attribute(("idx", pt_idx.to_string().as_str()));
    let v = BytesStart::new("c:v");
    let text = if prop == "values" {
        value
            .parse::<f64>()
            .map(|f| f.to_string())
            .unwrap_or_else(|_| value.to_string())
    } else {
        value.to_string()
    };
    let new_events = vec![
        Event::Start(pt),
        Event::Start(v),
        Event::Text(BytesText::from_escaped(escape(text))),
        Event::End(BytesEnd::new("c:v")),
        Event::End(BytesEnd::new("c:pt")),
    ];
    let insert_at = cache_range.1;
    for ev in new_events.into_iter().rev() {
        events.insert(insert_at, ev);
    }

    // Bump the ptCount attribute.
    let mut i = cache_range.0 + 1;
    while i < cache_range.1 {
        if let Event::Empty(e) = &events[i]
            && e.name().as_ref() == b"c:ptCount"
        {
            let count: usize = e
                .attributes()
                .flatten()
                .find(|a| a.key.as_ref() == b"val")
                .and_then(|a| String::from_utf8_lossy(&a.value).parse().ok())
                .unwrap_or(0);
            events[i] = Event::Empty(copy_attrs(e, b"val", &(count + 1).to_string()));
            break;
        }
        i += 1;
    }
    write_events(events)
}

/// Remove a chart point (`c:pt`) from a series' categories or values cache,
/// decrementing the `ptCount`. `remaining` is `chart.series[N].categories[K]`
/// or `chart.series[N].values[K]`.
pub fn remove_chart_point_lossless(
    xml_bytes: &[u8],
    remaining: &[path::PathSegment],
) -> AppResult<Vec<u8>> {
    let inner = if matches!(&remaining[0], path::PathSegment::Field(n) if n == "chart") {
        &remaining[1..]
    } else {
        remaining
    };
    let (ser_idx, prop, pt_idx) = match inner {
        [
            path::PathSegment::Field(n),
            path::PathSegment::Index(i),
            path::PathSegment::Field(p),
            path::PathSegment::Index(k),
        ] if n == "series" && (p == "categories" || p == "values") => (*i, p.as_str(), *k),
        _ => {
            return Err(AppError::PathParse(
                "Expected chart.series[N].categories[K] or .values[K]".to_string(),
            ));
        }
    };

    let mut events = read_events(xml_bytes)?;
    let (ct_start, ct_end) = find_chart_type_range(&events)
        .ok_or_else(|| AppError::PathParse("Chart type element not found".to_string()))?;
    let (ser_start, ser_end) = find_nth_elem_range(&events, b"c:ser", ct_start, ct_end, ser_idx)
        .ok_or_else(|| AppError::PathParse(format!("Series {ser_idx} not found")))?;

    let (target_name, cache_names): (&[u8], [&[u8]; 2]) = if prop == "categories" {
        (b"c:cat", [b"c:strCache", b"c:strLit"])
    } else {
        (b"c:val", [b"c:numCache", b"c:numLit"])
    };
    let target_range = find_elem_range(&events, target_name, ser_start)
        .filter(|r| r.0 <= ser_end)
        .ok_or_else(|| AppError::PathParse(format!("Series has no {prop}")))?;
    let cache_range = cache_names
        .iter()
        .find_map(|n| find_elem_range(&events, n, target_range.0).filter(|r| r.0 <= target_range.1))
        .ok_or_else(|| AppError::PathParse(format!("Series {prop} cache not found")))?;
    let (pt_start, pt_end) =
        find_nth_child_range(&events, cache_range.0, cache_range.1, b"c:pt", pt_idx)
            .ok_or_else(|| AppError::PathParse(format!("Chart point {pt_idx} not found")))?;
    for j in (pt_start..=pt_end).rev() {
        events.remove(j);
    }

    // Decrement the ptCount attribute.
    let mut i = cache_range.0 + 1;
    while i < cache_range.1 {
        if let Event::Empty(e) = &events[i]
            && e.name().as_ref() == b"c:ptCount"
        {
            let count: usize = e
                .attributes()
                .flatten()
                .find(|a| a.key.as_ref() == b"val")
                .and_then(|a| String::from_utf8_lossy(&a.value).parse().ok())
                .unwrap_or(0);
            let new_count = count.saturating_sub(1);
            events[i] = Event::Empty(copy_attrs(e, b"val", &new_count.to_string()));
            break;
        }
        i += 1;
    }
    write_events(events)
}
