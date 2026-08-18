use super::*;

pub fn replace_table_cell_property_lossless(
    xml_bytes: &[u8],
    shape_idx: usize,
    remaining: &[path::PathSegment],
    value: &str,
) -> AppResult<Vec<u8>> {
    let inner = if remaining.len() > 1
        && matches!(&remaining[0], path::PathSegment::Field(n) if n == "table")
    {
        &remaining[1..]
    } else {
        remaining
    };

    let (rest, row_idx, cell_idx) = match inner {
        [
            path::PathSegment::Field(rows),
            path::PathSegment::Index(row_idx),
            path::PathSegment::Field(cells),
            path::PathSegment::Index(cell_idx),
            rest @ ..,
        ] if rows.as_str() == "rows" && cells.as_str() == "cells" => (rest, *row_idx, *cell_idx),
        _ => {
            return Err(AppError::PathParse(
                "Expected table.rows[N].cells[M]".to_string(),
            ));
        }
    };

    let mut events = read_events(xml_bytes)?;
    let (shape_start, shape_end) =
        find_shape_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let tbl_range = find_elem_range(&events, b"a:tbl", shape_start)
        .filter(|r| r.0 <= shape_end)
        .ok_or_else(|| AppError::PathParse("Shape has no table".to_string()))?;
    let (row_start, row_end) =
        find_nth_child_range(&events, tbl_range.0, tbl_range.1, b"a:tr", row_idx)
            .ok_or_else(|| AppError::PathParse(format!("Table row {row_idx} not found")))?;
    let (cell_start, cell_end) =
        find_nth_child_range(&events, row_start, row_end, b"a:tc", cell_idx)
            .ok_or_else(|| AppError::PathParse(format!("Table cell {cell_idx} not found")))?;
    let (txbody_start, txbody_end) = find_elem_range(&events, b"a:txBody", cell_start)
        .filter(|r| r.0 <= cell_end)
        .ok_or_else(|| AppError::PathParse("Cell has no txBody".to_string()))?;

    if rest.len() == 1 && matches!(&rest[0], path::PathSegment::Field(n) if n == "text_frame") {
        let tf: crate::dto::TextFrameDto = serde_json::from_str(value)
            .map_err(|e| AppError::InvalidValue(format!("Invalid text_frame JSON: {e}")))?;
        let inner_events = read_events(crate::dto::xml::txbody_to_xml(&tf).as_bytes())?;
        events.splice(txbody_start + 1..txbody_end, inner_events);
        return write_events(events);
    }

    edit_txbody_path(&mut events, txbody_start, txbody_end, rest, value)?;
    write_events(events)
}

/// Generate the `<a:tr>` XML string for a new table row, padded with empty
/// cells up to `col_count`.
fn table_row_to_xml(row: &crate::dto::TableRowDto, col_count: usize) -> String {
    let mut writer = Writer::new(Vec::new());
    let mut tr = BytesStart::new("a:tr");
    if let Some(h) = row.height {
        tr.push_attribute(("h", h.to_string().as_str()));
    }
    writer.write_event(Event::Start(tr)).ok();
    for cell in &row.cells {
        let body: Vec<u8> = if let Some(ref tf) = cell.text_frame {
            let mut w2 = Writer::new(Vec::new());
            w2.write_event(Event::Start(BytesStart::new("a:txBody")))
                .ok();
            w2.write_event(Event::Start(BytesStart::new("a:bodyPr")))
                .ok();
            w2.write_event(Event::End(BytesEnd::new("a:bodyPr"))).ok();
            w2.write_event(Event::Empty(BytesStart::new("a:lstStyle")))
                .ok();
            for p in &tf.paragraphs {
                crate::dto::xml::write_paragraph(p, &mut w2);
            }
            w2.write_event(Event::End(BytesEnd::new("a:txBody"))).ok();
            w2.into_inner()
        } else {
            b"<a:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang=\"en-US\"/></a:p></a:txBody>".to_vec()
        };
        writer.get_mut().write_all(b"<a:tc>").ok();
        writer.get_mut().write_all(&body).ok();
        writer.get_mut().write_all(b"</a:tc>").ok();
    }
    // Pad with empty cells up to the column count.
    for _ in row.cells.len()..col_count {
        writer
            .get_mut()
            .write_all(
                b"<a:tc><a:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang=\"en-US\"/></a:p></a:txBody></a:tc>",
            )
            .ok();
    }
    writer.write_event(Event::End(BytesEnd::new("a:tr"))).ok();
    String::from_utf8(writer.into_inner()).expect("valid UTF-8")
}

/// Add a table row (`table.rows` appends, `table.rows[N]` inserts after N).
pub fn add_table_row_lossless(
    xml_bytes: &[u8],
    shape_idx: usize,
    remaining: &[path::PathSegment],
    value_json: &str,
) -> AppResult<Vec<u8>> {
    let inner = if remaining.len() > 1
        && matches!(&remaining[0], path::PathSegment::Field(n) if n == "table")
    {
        &remaining[1..]
    } else {
        remaining
    };
    let insert_after = match inner {
        [path::PathSegment::Field(n), path::PathSegment::Index(i)] if n == "rows" => Some(*i),
        [path::PathSegment::Field(n)] if n == "rows" => None,
        _ => {
            return Err(AppError::PathParse(
                "Expected table.rows or table.rows[N]".to_string(),
            ));
        }
    };

    let row: crate::dto::TableRowDto = serde_json::from_str(value_json)
        .map_err(|e| AppError::InvalidValue(format!("Invalid row JSON: {e}")))?;

    let mut events = read_events(xml_bytes)?;
    let (tbl_start, tbl_end) =
        find_table_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let col_count = count_grid_cols(&events, tbl_start, tbl_end);
    let new_row = table_row_to_xml(&row, col_count);
    let new_events = read_events(new_row.as_bytes())?;

    // Insert at the given index: the new row becomes the (insert_after)th row,
    // with everything from that position down shifting by one. An index past
    // the current row count appends before the end of the table.
    let insert_pos = if let Some(at) = insert_after {
        let mut count = 0usize;
        let mut pos = tbl_end;
        let mut i = tbl_start + 1;
        while i < tbl_end {
            if let Event::Start(e) = &events[i]
                && e.name().as_ref() == b"a:tr"
            {
                if count == at {
                    pos = i;
                    break;
                }
                count += 1;
            }
            i += 1;
        }
        pos
    } else {
        tbl_end
    };

    for ev in new_events.into_iter().rev() {
        events.insert(insert_pos, ev);
    }
    write_events(events)
}

/// Remove a table row (`table.rows[N]`).
pub fn remove_table_row_lossless(
    xml_bytes: &[u8],
    shape_idx: usize,
    remaining: &[path::PathSegment],
) -> AppResult<Vec<u8>> {
    let inner = if remaining.len() > 1
        && matches!(&remaining[0], path::PathSegment::Field(n) if n == "table")
    {
        &remaining[1..]
    } else {
        remaining
    };
    let row_idx = match inner {
        [path::PathSegment::Field(n), path::PathSegment::Index(i)] if n == "rows" => *i,
        _ => {
            return Err(AppError::PathParse("Expected table.rows[N]".to_string()));
        }
    };

    let mut events = read_events(xml_bytes)?;
    let (tbl_start, tbl_end) =
        find_table_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let (row_start, row_end) = find_nth_child_range(&events, tbl_start, tbl_end, b"a:tr", row_idx)
        .ok_or_else(|| AppError::PathParse(format!("Table row {row_idx} not found")))?;
    for j in (row_start..=row_end).rev() {
        events.remove(j);
    }
    write_events(events)
}

/// Add a table column (`table.grid` appends, `table.grid[N]` inserts after N).
/// Inserts a `<a:gridCol>` into the table grid and an empty `<a:tc>` into every
/// row.
pub fn add_table_column_lossless(
    xml_bytes: &[u8],
    shape_idx: usize,
    remaining: &[path::PathSegment],
    value_json: &str,
) -> AppResult<Vec<u8>> {
    let inner = if remaining.len() > 1
        && matches!(&remaining[0], path::PathSegment::Field(n) if n == "table")
    {
        &remaining[1..]
    } else {
        remaining
    };
    let insert_after = match inner {
        [path::PathSegment::Field(n), path::PathSegment::Index(i)] if n == "grid" => Some(*i),
        [path::PathSegment::Field(n)] if n == "grid" => None,
        _ => {
            return Err(AppError::PathParse(
                "Expected table.grid or table.grid[N]".to_string(),
            ));
        }
    };

    let col: crate::dto::GridColDto = serde_json::from_str(value_json)
        .map_err(|e| AppError::InvalidValue(format!("Invalid column JSON: {e}")))?;

    let mut events = read_events(xml_bytes)?;
    let (tbl_start, tbl_end) =
        find_table_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let (grid_start, grid_end) = find_elem_range(&events, b"a:tblGrid", tbl_start)
        .filter(|r| r.0 <= tbl_end)
        .ok_or_else(|| AppError::PathParse("Table has no grid".to_string()))?;

    // Insert at the given index: the new column becomes the (insert_after)th
    // grid column. An index past the current column count appends at the end
    // of the grid.
    let grid_insert_pos = if let Some(at) = insert_after {
        let mut count = 0usize;
        let mut pos = grid_end;
        let mut i = grid_start + 1;
        while i < grid_end {
            let is_grid_col = matches!(
                &events[i],
                Event::Start(e) | Event::Empty(e) if e.name().as_ref() == b"a:gridCol"
            );
            if is_grid_col {
                if count == at {
                    pos = i;
                    break;
                }
                count += 1;
            }
            i += 1;
        }
        pos
    } else {
        grid_end
    };

    let mut grid_col = BytesStart::new("a:gridCol");
    grid_col.push_attribute(("w", col.width.to_string().as_str()));

    // Insert gridCol.
    events.insert(grid_insert_pos, Event::Empty(grid_col));

    // Insert an empty a:tc into every row.
    let empty_cell = "<a:tc><a:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang=\"en-US\"/></a:p></a:txBody></a:tc>".to_string();
    let cell_events = read_events(empty_cell.as_bytes())?;
    let mut i = tbl_start + 1;
    while i < tbl_end {
        if let Event::Start(e) = &events[i]
            && e.name().as_ref() == b"a:tr"
        {
            let (_, row_end) = find_elem_range(&events, b"a:tr", i).unwrap();
            for ev in cell_events.iter().rev() {
                events.insert(row_end, ev.clone());
            }
            i = row_end + 1;
        } else {
            i += 1;
        }
    }

    write_events(events)
}

/// Remove a table column (`table.grid[N]`). Removes the Nth `<a:gridCol>` and
/// the Nth `<a:tc>` from every row.
pub fn remove_table_column_lossless(
    xml_bytes: &[u8],
    shape_idx: usize,
    remaining: &[path::PathSegment],
) -> AppResult<Vec<u8>> {
    let inner = if remaining.len() > 1
        && matches!(&remaining[0], path::PathSegment::Field(n) if n == "table")
    {
        &remaining[1..]
    } else {
        remaining
    };
    let col_idx = match inner {
        [path::PathSegment::Field(n), path::PathSegment::Index(i)] if n == "grid" => *i,
        _ => {
            return Err(AppError::PathParse("Expected table.grid[N]".to_string()));
        }
    };

    let mut events = read_events(xml_bytes)?;
    let (tbl_start, tbl_end) =
        find_table_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let (grid_start, grid_end) = find_elem_range(&events, b"a:tblGrid", tbl_start)
        .filter(|r| r.0 <= tbl_end)
        .ok_or_else(|| AppError::PathParse("Table has no grid".to_string()))?;

    // Remove the Nth gridCol.
    let mut count = 0usize;
    let mut i = grid_start + 1;
    let mut removed_col = false;
    while i < grid_end {
        let is_col = matches!(&events[i], Event::Start(e) | Event::Empty(e) if e.name().as_ref() == b"a:gridCol");
        if is_col {
            if count == col_idx {
                events.remove(i);
                removed_col = true;
                break;
            }
            count += 1;
        }
        i += 1;
    }
    if !removed_col {
        return Err(AppError::PathParse(format!(
            "Table column {col_idx} not found"
        )));
    }

    // Removing the gridCol shifted subsequent indices; re-locate the table.
    let (tbl_start, tbl_end) =
        find_table_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;

    // Remove the Nth a:tc from every row. Collect row ranges first and process
    // in reverse so earlier indices stay stable while we remove cells.
    let mut rows = Vec::new();
    let mut i = tbl_start + 1;
    while i < tbl_end {
        if let Event::Start(e) = &events[i]
            && e.name().as_ref() == b"a:tr"
        {
            let (row_start, row_end) = find_elem_range(&events, b"a:tr", i).unwrap();
            rows.push((row_start, row_end));
            i = row_end + 1;
        } else {
            i += 1;
        }
    }
    for (row_start, row_end) in rows.into_iter().rev() {
        let (cell_start, cell_end) = find_nth_child_range(
            &events, row_start, row_end, b"a:tc", col_idx,
        )
        .ok_or_else(|| AppError::PathParse(format!("Table cell {col_idx} not found in row")))?;
        for j in (cell_start..=cell_end).rev() {
            events.remove(j);
        }
    }

    write_events(events)
}

/// Replace the whole table (`a:tbl`) of a table shape.
pub fn replace_whole_table_lossless(
    xml_bytes: &[u8],
    shape_idx: usize,
    value: &str,
) -> AppResult<Vec<u8>> {
    let table: crate::dto::TableDto = serde_json::from_str(value)
        .map_err(|e| AppError::InvalidValue(format!("Invalid table JSON: {e}")))?;
    let inner_events = read_events(crate::dto::xml::table_to_xml(&table).as_bytes())?;

    let mut events = read_events(xml_bytes)?;
    let (shape_start, shape_end) =
        find_shape_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let (tbl_start, tbl_end) = find_elem_range(&events, b"a:tbl", shape_start)
        .filter(|r| r.0 <= shape_end)
        .ok_or_else(|| AppError::PathParse("Shape has no table".to_string()))?;
    events.splice(tbl_start..=tbl_end, inner_events);
    write_events(events)
}

/// Replace a single table row (`a:tr`) with a freshly serialized `TableRowDto`,
/// padded to the table's current column count.
pub fn replace_table_row_lossless(
    xml_bytes: &[u8],
    shape_idx: usize,
    row_idx: usize,
    value: &str,
) -> AppResult<Vec<u8>> {
    let row: crate::dto::TableRowDto = serde_json::from_str(value)
        .map_err(|e| AppError::InvalidValue(format!("Invalid table row JSON: {e}")))?;
    let mut events = read_events(xml_bytes)?;
    let (shape_start, shape_end) =
        find_shape_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let (tbl_start, tbl_end) = find_elem_range(&events, b"a:tbl", shape_start)
        .filter(|r| r.0 <= shape_end)
        .ok_or_else(|| AppError::PathParse("Shape has no table".to_string()))?;
    let col_count = count_grid_cols(&events, tbl_start, tbl_end);
    let (row_start, row_end) = find_nth_child_range(&events, tbl_start, tbl_end, b"a:tr", row_idx)
        .ok_or_else(|| AppError::PathParse(format!("Table row {row_idx} not found")))?;
    let inner_events = read_events(table_row_to_xml(&row, col_count).as_bytes())?;
    events.splice(row_start..=row_end, inner_events);
    write_events(events)
}

/// Replace a single table cell (`a:tc`) with a freshly serialized `TableCellDto`.
pub fn replace_table_cell_lossless(
    xml_bytes: &[u8],
    shape_idx: usize,
    row_idx: usize,
    cell_idx: usize,
    value: &str,
) -> AppResult<Vec<u8>> {
    let cell: crate::dto::TableCellDto = serde_json::from_str(value)
        .map_err(|e| AppError::InvalidValue(format!("Invalid table cell JSON: {e}")))?;
    let inner_events = read_events(crate::dto::xml::table_cell_to_xml(&cell).as_bytes())?;

    let mut events = read_events(xml_bytes)?;
    let (shape_start, shape_end) =
        find_shape_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let (tbl_start, tbl_end) = find_elem_range(&events, b"a:tbl", shape_start)
        .filter(|r| r.0 <= shape_end)
        .ok_or_else(|| AppError::PathParse("Shape has no table".to_string()))?;
    let (row_start, row_end) = find_nth_child_range(&events, tbl_start, tbl_end, b"a:tr", row_idx)
        .ok_or_else(|| AppError::PathParse(format!("Table row {row_idx} not found")))?;
    let (cell_start, cell_end) =
        find_nth_child_range(&events, row_start, row_end, b"a:tc", cell_idx)
            .ok_or_else(|| AppError::PathParse(format!("Table cell {cell_idx} not found")))?;
    events.splice(cell_start..=cell_end, inner_events);
    write_events(events)
}

/// Replace a single table grid column (`a:gridCol`).
pub fn replace_table_grid_col_lossless(
    xml_bytes: &[u8],
    shape_idx: usize,
    col_idx: usize,
    value: &str,
) -> AppResult<Vec<u8>> {
    let col: crate::dto::GridColDto = serde_json::from_str(value)
        .map_err(|e| AppError::InvalidValue(format!("Invalid grid column JSON: {e}")))?;
    let inner_events = read_events(crate::dto::xml::grid_col_to_xml(&col).as_bytes())?;

    let mut events = read_events(xml_bytes)?;
    let (shape_start, shape_end) =
        find_shape_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let (tbl_start, tbl_end) = find_elem_range(&events, b"a:tbl", shape_start)
        .filter(|r| r.0 <= shape_end)
        .ok_or_else(|| AppError::PathParse("Shape has no table".to_string()))?;
    let (grid_start, grid_end) = find_elem_range(&events, b"a:tblGrid", tbl_start)
        .filter(|r| r.0 <= tbl_end)
        .ok_or_else(|| AppError::PathParse("Table has no grid".to_string()))?;
    let (col_start, col_end) =
        find_nth_child_range(&events, grid_start, grid_end, b"a:gridCol", col_idx)
            .ok_or_else(|| AppError::PathParse(format!("Grid column {col_idx} not found")))?;
    events.splice(col_start..=col_end, inner_events);
    write_events(events)
}
