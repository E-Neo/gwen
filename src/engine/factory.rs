use quick_xml::Writer;
use std::io::Cursor;
use std::io::Write;

use crate::dto::{AddShape, ShapeTypeInput};
use crate::error::AppResult;

pub fn generate_shape_xml(shape: &AddShape, new_id: u32) -> AppResult<Vec<u8>> {
    match shape.shape_type {
        ShapeTypeInput::Textbox => generate_textbox_xml(shape, new_id),
        ShapeTypeInput::Picture => generate_picture_xml(shape, new_id),
        ShapeTypeInput::Table => generate_table_xml(shape, new_id),
        ShapeTypeInput::Chart => generate_chart_xml(shape, new_id),
    }
}

fn generate_table_xml(shape: &AddShape, new_id: u32) -> AppResult<Vec<u8>> {
    let left = shape.left.unwrap_or(0);
    let top = shape.top.unwrap_or(0);
    let width = shape.width.unwrap_or(6096000);
    let height = shape.height.unwrap_or(3048000);
    let name = format!("Table {}", new_id);
    let table = shape.table.as_ref().ok_or_else(|| {
        crate::error::AppError::InvalidValue("table definition required".to_string())
    })?;

    let mut writer = Writer::new(Cursor::new(Vec::new()));
    write_open_tag(&mut writer, "p:graphicFrame", &[]);

    write_open_tag(&mut writer, "p:nvGraphicFramePr", &[]);
    write_empty_tag(
        &mut writer,
        "p:cNvPr",
        &[("id", &new_id.to_string()), ("name", &name)],
    );
    write_empty_tag(&mut writer, "p:cNvGraphicFramePr", &[]);
    write_empty_tag(&mut writer, "p:nvPr", &[]);
    write_close_tag(&mut writer, "p:nvGraphicFramePr");

    write_open_tag(&mut writer, "p:xfrm", &[]);
    write_empty_tag(
        &mut writer,
        "a:off",
        &[("x", &left.to_string()), ("y", &top.to_string())],
    );
    write_empty_tag(
        &mut writer,
        "a:ext",
        &[("cx", &width.to_string()), ("cy", &height.to_string())],
    );
    write_close_tag(&mut writer, "p:xfrm");

    write_open_tag(&mut writer, "a:graphic", &[]);
    write_open_tag(
        &mut writer,
        "a:graphicData",
        &[(
            "uri",
            "http://schemas.openxmlformats.org/drawingml/2006/table",
        )],
    );
    write_open_tag(&mut writer, "a:tbl", &[]);

    write_open_tag(&mut writer, "a:tblPr", &[]);
    write_empty_tag(&mut writer, "a:noFill", &[]);
    write_close_tag(&mut writer, "a:tblPr");

    write_open_tag(&mut writer, "a:tblGrid", &[]);
    for col in &table.grid {
        write_empty_tag(&mut writer, "a:gridCol", &[("w", &col.width.to_string())]);
    }
    write_close_tag(&mut writer, "a:tblGrid");

    for row in &table.rows {
        let h_str = row.height.map(|h| h.to_string());
        let mut row_attrs: Vec<(&str, &str)> = Vec::new();
        if let Some(ref h) = h_str {
            row_attrs.push(("h", h));
        }
        write_open_tag(&mut writer, "a:tr", &row_attrs);
        for cell in &row.cells {
            let rs_str = cell.row_span.map(|v| v.to_string());
            let gs_str = cell.grid_span.map(|v| v.to_string());
            let mut cell_attrs: Vec<(&str, &str)> = Vec::new();
            if let Some(ref rs) = rs_str {
                cell_attrs.push(("rowSpan", rs));
            }
            if let Some(ref gs) = gs_str {
                cell_attrs.push(("gridSpan", gs));
            }
            write_open_tag(&mut writer, "a:tc", &cell_attrs);
            if let Some(ref tf) = cell.text_frame {
                writer
                    .get_mut()
                    .write_all(b"<a:txBody>")
                    .map_err(crate::error::AppError::Io)?;
                let txbody = crate::dto::xml::txbody_to_xml(tf);
                writer
                    .get_mut()
                    .write_all(txbody.as_bytes())
                    .map_err(crate::error::AppError::Io)?;
                writer
                    .get_mut()
                    .write_all(b"</a:txBody>")
                    .map_err(crate::error::AppError::Io)?;
            } else {
                write_open_tag(&mut writer, "a:txBody", &[]);
                write_empty_tag(&mut writer, "a:bodyPr", &[]);
                write_open_tag(&mut writer, "a:p", &[]);
                write_close_tag(&mut writer, "a:p");
                write_close_tag(&mut writer, "a:txBody");
            }
            write_close_tag(&mut writer, "a:tc");
        }
        write_close_tag(&mut writer, "a:tr");
    }

    write_close_tag(&mut writer, "a:tbl");
    write_close_tag(&mut writer, "a:graphicData");
    write_close_tag(&mut writer, "a:graphic");
    write_close_tag(&mut writer, "p:graphicFrame");

    let inner = writer.into_inner().into_inner();
    Ok(inner)
}

fn generate_chart_xml(shape: &AddShape, new_id: u32) -> AppResult<Vec<u8>> {
    let left = shape.left.unwrap_or(0);
    let top = shape.top.unwrap_or(0);
    let width = shape.width.unwrap_or(6096000);
    let height = shape.height.unwrap_or(3048000);
    let name = format!("Chart {}", new_id);
    let r_id = shape.r_id.as_deref().unwrap_or("rId1");

    let mut writer = Writer::new(Cursor::new(Vec::new()));
    write_open_tag(&mut writer, "p:graphicFrame", &[]);
    write_open_tag(&mut writer, "p:nvGraphicFramePr", &[]);
    write_empty_tag(
        &mut writer,
        "p:cNvPr",
        &[("id", &new_id.to_string()), ("name", &name)],
    );
    write_empty_tag(&mut writer, "p:cNvGraphicFramePr", &[]);
    write_empty_tag(&mut writer, "p:nvPr", &[]);
    write_close_tag(&mut writer, "p:nvGraphicFramePr");
    write_open_tag(&mut writer, "p:xfrm", &[]);
    write_empty_tag(
        &mut writer,
        "a:off",
        &[("x", &left.to_string()), ("y", &top.to_string())],
    );
    write_empty_tag(
        &mut writer,
        "a:ext",
        &[("cx", &width.to_string()), ("cy", &height.to_string())],
    );
    write_close_tag(&mut writer, "p:xfrm");
    write_open_tag(&mut writer, "a:graphic", &[]);
    write_open_tag(
        &mut writer,
        "a:graphicData",
        &[(
            "uri",
            "http://schemas.openxmlformats.org/drawingml/2006/chart",
        )],
    );
    write_empty_tag(&mut writer, "c:chart", &[("r:id", r_id)]);
    write_close_tag(&mut writer, "a:graphicData");
    write_close_tag(&mut writer, "a:graphic");
    write_close_tag(&mut writer, "p:graphicFrame");

    let inner = writer.into_inner().into_inner();
    Ok(inner)
}

fn generate_textbox_xml(shape: &AddShape, new_id: u32) -> AppResult<Vec<u8>> {
    let left = shape.left.unwrap_or(0);
    let top = shape.top.unwrap_or(0);
    let width = shape.width.unwrap_or(914400);
    let height = shape.height.unwrap_or(274320);
    let text = shape.text.as_deref().unwrap_or("");
    let name = format!("TextBox {}", new_id);

    let mut writer = Writer::new(Cursor::new(Vec::new()));
    write_open_tag(&mut writer, "p:sp", &[]);

    write_open_tag(&mut writer, "p:nvSpPr", &[]);
    write_empty_tag(
        &mut writer,
        "p:cNvPr",
        &[("id", &new_id.to_string()), ("name", &name)],
    );
    write_empty_tag(&mut writer, "p:cNvSpPr", &[("txBox", "1")]);
    write_empty_tag(&mut writer, "p:nvPr", &[]);
    write_close_tag(&mut writer, "p:nvSpPr");

    write_open_tag(&mut writer, "p:spPr", &[]);
    write_open_tag(&mut writer, "a:xfrm", &[]);
    write_empty_tag(
        &mut writer,
        "a:off",
        &[("x", &left.to_string()), ("y", &top.to_string())],
    );
    write_empty_tag(
        &mut writer,
        "a:ext",
        &[("cx", &width.to_string()), ("cy", &height.to_string())],
    );
    write_close_tag(&mut writer, "a:xfrm");
    write_empty_tag(&mut writer, "a:prstGeom", &[("prst", "rect")]);
    write_empty_tag(&mut writer, "a:noFill", &[]);
    write_close_tag(&mut writer, "p:spPr");

    write_open_tag(&mut writer, "p:txBody", &[]);
    write_empty_tag(&mut writer, "a:bodyPr", &[]);
    write_empty_tag(&mut writer, "a:lstStyle", &[]);
    write_open_tag(&mut writer, "a:p", &[]);
    write_open_tag(&mut writer, "a:r", &[]);
    write_empty_tag(&mut writer, "a:rPr", &[("lang", "en-US"), ("sz", "1200")]);
    write_text_tag(&mut writer, "a:t", text);
    write_close_tag(&mut writer, "a:r");
    write_close_tag(&mut writer, "a:p");
    write_close_tag(&mut writer, "p:txBody");

    write_close_tag(&mut writer, "p:sp");

    let inner = writer.into_inner().into_inner();
    Ok(inner)
}

fn generate_picture_xml(shape: &AddShape, new_id: u32) -> AppResult<Vec<u8>> {
    let left = shape.left.unwrap_or(0);
    let top = shape.top.unwrap_or(0);
    let width = shape.width.unwrap_or(1000000);
    let height = shape.height.unwrap_or(800000);
    let name = format!("Picture {}", new_id);
    let r_id = "rId1";

    let mut writer = Writer::new(Cursor::new(Vec::new()));
    write_open_tag(&mut writer, "p:pic", &[]);

    write_open_tag(&mut writer, "p:nvPicPr", &[]);
    write_empty_tag(
        &mut writer,
        "p:cNvPr",
        &[("id", &new_id.to_string()), ("name", &name)],
    );
    write_empty_tag(&mut writer, "p:cNvPicPr", &[]);
    write_empty_tag(&mut writer, "p:nvPr", &[]);
    write_close_tag(&mut writer, "p:nvPicPr");

    write_open_tag(&mut writer, "p:blipFill", &[]);
    write_empty_tag(&mut writer, "a:blip", &[("r:embed", r_id)]);
    write_open_tag(&mut writer, "a:stretch", &[]);
    write_empty_tag(&mut writer, "a:fillRect", &[]);
    write_close_tag(&mut writer, "a:stretch");
    write_close_tag(&mut writer, "p:blipFill");

    write_open_tag(&mut writer, "p:spPr", &[]);
    write_open_tag(&mut writer, "a:xfrm", &[]);
    write_empty_tag(
        &mut writer,
        "a:off",
        &[("x", &left.to_string()), ("y", &top.to_string())],
    );
    write_empty_tag(
        &mut writer,
        "a:ext",
        &[("cx", &width.to_string()), ("cy", &height.to_string())],
    );
    write_close_tag(&mut writer, "a:xfrm");
    write_empty_tag(&mut writer, "a:prstGeom", &[("prst", "rect")]);
    write_close_tag(&mut writer, "p:spPr");

    write_close_tag(&mut writer, "p:pic");

    let inner = writer.into_inner().into_inner();
    Ok(inner)
}

pub fn find_max_shape_id(xml_bytes: &[u8]) -> u32 {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true);
    let mut max_id = 0u32;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e)) => {
                if e.name().as_ref() == b"p:cNvPr" {
                    for attr in e.attributes() {
                        if let Ok(a) = attr
                            && a.key.as_ref() == b"id"
                            && let Ok(v) = String::from_utf8_lossy(&a.value).parse::<u32>()
                            && v > max_id
                        {
                            max_id = v;
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            _ => {}
        }
    }
    max_id
}

fn write_open_tag(writer: &mut Writer<Cursor<Vec<u8>>>, name: &str, attrs: &[(&str, &str)]) {
    use quick_xml::events::{BytesStart, Event};
    let mut elem = BytesStart::new(name);
    for (k, v) in attrs {
        elem.push_attribute((*k, *v));
    }
    writer.write_event(Event::Start(elem)).unwrap();
}

fn write_close_tag(writer: &mut Writer<Cursor<Vec<u8>>>, name: &str) {
    use quick_xml::events::{BytesEnd, Event};
    writer.write_event(Event::End(BytesEnd::new(name))).unwrap();
}

fn write_empty_tag(writer: &mut Writer<Cursor<Vec<u8>>>, name: &str, attrs: &[(&str, &str)]) {
    use quick_xml::events::{BytesStart, Event};
    let mut elem = BytesStart::new(name);
    for (k, v) in attrs {
        elem.push_attribute((*k, *v));
    }
    writer.write_event(Event::Empty(elem)).unwrap();
}

fn write_text_tag(writer: &mut Writer<Cursor<Vec<u8>>>, name: &str, text: &str) {
    use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
    writer
        .write_event(Event::Start(BytesStart::new(name)))
        .unwrap();
    writer
        .write_event(Event::Text(BytesText::new(text)))
        .unwrap();
    writer.write_event(Event::End(BytesEnd::new(name))).unwrap();
}
