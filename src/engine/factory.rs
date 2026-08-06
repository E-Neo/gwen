use quick_xml::Writer;
use std::io::Cursor;
use std::io::Write;

use crate::dto::{AddShape, FillType, ShapeTypeInput, SlideDto};
use crate::error::AppResult;

pub fn generate_shape_xml(shape: &AddShape, new_id: u32) -> AppResult<Vec<u8>> {
    match shape.shape_type {
        ShapeTypeInput::Textbox => generate_textbox_xml(shape, new_id),
        ShapeTypeInput::Table => generate_table_xml(shape, new_id),
        ShapeTypeInput::AutoShape => generate_autoshape_xml(shape, new_id),
    }
}

fn generate_table_xml(shape: &AddShape, new_id: u32) -> AppResult<Vec<u8>> {
    let left = shape.left.unwrap_or(0);
    let top = shape.top.unwrap_or(0);
    let width = shape.width.unwrap_or(6096000);
    let height = shape.height.unwrap_or(3048000);
    let name = shape
        .name
        .clone()
        .unwrap_or_else(|| format!("Table {}", new_id));
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

fn generate_textbox_xml(shape: &AddShape, new_id: u32) -> AppResult<Vec<u8>> {
    let left = shape.left.unwrap_or(0);
    let top = shape.top.unwrap_or(0);
    let width = shape.width.unwrap_or(914400);
    let height = shape.height.unwrap_or(274320);
    let text = shape.text.as_deref().unwrap_or("");
    let name = shape
        .name
        .clone()
        .unwrap_or_else(|| format!("TextBox {}", new_id));

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
    write_fill_xml(shape, &mut writer)?;
    write_outline_xml(shape, &mut writer)?;
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

fn generate_autoshape_xml(shape: &AddShape, new_id: u32) -> AppResult<Vec<u8>> {
    let left = shape.left.unwrap_or(0);
    let top = shape.top.unwrap_or(0);
    let width = shape.width.unwrap_or(914400);
    let height = shape.height.unwrap_or(274320);
    let text = shape.text.as_deref().unwrap_or("");
    let name = shape
        .name
        .clone()
        .unwrap_or_else(|| format!("AutoShape {}", new_id));
    let prst = shape.auto_shape_type.as_deref().unwrap_or("rect");

    let mut writer = Writer::new(Cursor::new(Vec::new()));
    write_open_tag(&mut writer, "p:sp", &[]);

    write_open_tag(&mut writer, "p:nvSpPr", &[]);
    write_empty_tag(
        &mut writer,
        "p:cNvPr",
        &[("id", &new_id.to_string()), ("name", &name)],
    );
    write_empty_tag(&mut writer, "p:cNvSpPr", &[]);
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
    write_empty_tag(&mut writer, "a:prstGeom", &[("prst", prst)]);
    write_fill_xml(shape, &mut writer)?;
    write_outline_xml(shape, &mut writer)?;
    write_close_tag(&mut writer, "p:spPr");

    if !text.is_empty() {
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
    }

    write_close_tag(&mut writer, "p:sp");

    let inner = writer.into_inner().into_inner();
    Ok(inner)
}

fn write_fill_xml(shape: &AddShape, writer: &mut Writer<Cursor<Vec<u8>>>) -> AppResult<()> {
    if let Some(ref fill) = shape.fill {
        match fill.fill_type {
            Some(FillType::NoFill) => {
                write_empty_tag(writer, "a:noFill", &[]);
            }
            _ => {
                write_open_tag(writer, "a:solidFill", &[]);
                if let Some(ref color) = fill.color {
                    if let Some(ref rgb) = color.rgb {
                        write_empty_tag(writer, "a:srgbClr", &[("val", rgb)]);
                    } else if let Some(ref tc) = color.theme_color {
                        write_empty_tag(writer, "a:schemeClr", &[("val", tc)]);
                    } else {
                        write_empty_tag(writer, "a:srgbClr", &[("val", "4472C4")]);
                    }
                } else {
                    write_empty_tag(writer, "a:srgbClr", &[("val", "4472C4")]);
                }
                write_close_tag(writer, "a:solidFill");
            }
        }
    } else {
        write_open_tag(writer, "a:solidFill", &[]);
        write_empty_tag(writer, "a:srgbClr", &[("val", "4472C4")]);
        write_close_tag(writer, "a:solidFill");
    }
    Ok(())
}

fn write_outline_xml(shape: &AddShape, writer: &mut Writer<Cursor<Vec<u8>>>) -> AppResult<()> {
    if let Some(ref outline) = shape.outline {
        let w_str = outline.width.map(|w| w.to_string());
        let has_any = outline.width.is_some()
            || outline.cap.is_some()
            || outline.compound.is_some()
            || outline.dash.is_some()
            || outline.fill.is_some();
        if !has_any {
            return Ok(());
        }
        let mut attr_pairs: Vec<(&str, &str)> = Vec::new();
        if let Some(ref ws) = w_str {
            attr_pairs.push(("w", ws.as_str()));
        }
        if let Some(ref cap) = outline.cap {
            attr_pairs.push((
                "cap",
                match cap {
                    crate::dto::LineCap::Rnd => "rnd",
                    crate::dto::LineCap::Sq => "sq",
                    crate::dto::LineCap::Flat => "flat",
                },
            ));
        }
        if let Some(ref cmp) = outline.compound {
            attr_pairs.push((
                "cmpd",
                match cmp {
                    crate::dto::CompoundLine::Sng => "sng",
                    crate::dto::CompoundLine::Dbl => "dbl",
                    crate::dto::CompoundLine::ThickThin => "thickThin",
                    crate::dto::CompoundLine::ThinThick => "thinThick",
                    crate::dto::CompoundLine::Tri => "tri",
                },
            ));
        }
        write_open_tag(writer, "a:ln", &attr_pairs);
        if let Some(ref dash) = outline.dash {
            write_empty_tag(
                writer,
                "a:prstDash",
                &[(
                    "val",
                    match dash {
                        crate::dto::LineDash::Solid => "solid",
                        crate::dto::LineDash::Dot => "dot",
                        crate::dto::LineDash::Dash => "dash",
                        crate::dto::LineDash::LgDash => "lgDash",
                        crate::dto::LineDash::DashDot => "dashDot",
                        crate::dto::LineDash::LgDashDot => "lgDashDot",
                        crate::dto::LineDash::LgDashDotDot => "lgDashDotDot",
                        crate::dto::LineDash::SysDash => "sysDash",
                        crate::dto::LineDash::SysDot => "sysDot",
                        crate::dto::LineDash::SysDashDot => "sysDashDot",
                        crate::dto::LineDash::SysDashDotDot => "sysDashDotDot",
                    },
                )],
            );
        }
        if let Some(ref fill) = outline.fill {
            match fill.fill_type {
                Some(FillType::NoFill) => {
                    write_empty_tag(writer, "a:noFill", &[]);
                }
                _ => {
                    write_open_tag(writer, "a:solidFill", &[]);
                    if let Some(ref color) = fill.color {
                        if let Some(ref rgb) = color.rgb {
                            write_empty_tag(writer, "a:srgbClr", &[("val", rgb)]);
                        } else if let Some(ref tc) = color.theme_color {
                            write_empty_tag(writer, "a:schemeClr", &[("val", tc)]);
                        }
                    }
                    write_close_tag(writer, "a:solidFill");
                }
            }
        }
        write_close_tag(writer, "a:ln");
    }
    Ok(())
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

/// Generate a notes slide (`p:notes`) part with the given shapes. When no
/// shapes are supplied, the standard notes placeholders (slide image, notes
/// body, slide number) are created so that `notes_text_frame` resolves.
pub fn generate_notes_xml(slide: &SlideDto) -> AppResult<Vec<u8>> {
    use quick_xml::events::BytesStart;
    let mut writer = Writer::new(Cursor::new(Vec::new()));

    let mut notes = BytesStart::new("p:notes");
    notes.push_attribute((
        "xmlns:a",
        "http://schemas.openxmlformats.org/drawingml/2006/main",
    ));
    notes.push_attribute((
        "xmlns:r",
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
    ));
    notes.push_attribute((
        "xmlns:p",
        "http://schemas.openxmlformats.org/presentationml/2006/main",
    ));
    write_open_tag_full(&mut writer, &notes);

    write_open_tag(&mut writer, "p:cSld", &[]);
    write_open_tag(&mut writer, "p:spTree", &[]);

    write_open_tag(&mut writer, "p:nvGrpSpPr", &[]);
    write_empty_tag(&mut writer, "p:cNvPr", &[("id", "1"), ("name", "")]);
    write_empty_tag(&mut writer, "p:cNvGrpSpPr", &[]);
    write_empty_tag(&mut writer, "p:nvPr", &[]);
    write_close_tag(&mut writer, "p:nvGrpSpPr");

    write_open_tag(&mut writer, "p:grpSpPr", &[]);
    write_open_tag(&mut writer, "a:xfrm", &[]);
    write_empty_tag(&mut writer, "a:off", &[("x", "0"), ("y", "0")]);
    write_empty_tag(&mut writer, "a:ext", &[("cx", "0"), ("cy", "0")]);
    write_empty_tag(&mut writer, "a:chOff", &[("x", "0"), ("y", "0")]);
    write_empty_tag(&mut writer, "a:chExt", &[("cx", "0"), ("cy", "0")]);
    write_close_tag(&mut writer, "a:xfrm");
    write_close_tag(&mut writer, "p:grpSpPr");

    if slide.shapes.is_empty() {
        write_notes_placeholder(&mut writer, 2, "Slide Image Placeholder 1", "sldImg", 2);
        write_notes_body_placeholder(&mut writer);
        write_notes_placeholder(&mut writer, 4, "Slide Number Placeholder 3", "sldNum", 5);
    } else {
        for shape in &slide.shapes {
            let xml = crate::dto::xml::shape_to_xml(shape);
            writer
                .get_mut()
                .write_all(xml.as_bytes())
                .map_err(crate::error::AppError::Io)?;
        }
    }

    write_close_tag(&mut writer, "p:spTree");
    write_close_tag(&mut writer, "p:cSld");
    write_close_tag(&mut writer, "p:notes");

    let inner = writer.into_inner().into_inner();
    Ok(inner)
}

fn write_notes_placeholder(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    id: u32,
    name: &str,
    ph_type: &str,
    idx: u32,
) {
    write_open_tag(writer, "p:sp", &[]);
    write_open_tag(writer, "p:nvSpPr", &[]);
    write_empty_tag(
        writer,
        "p:cNvPr",
        &[("id", &id.to_string()), ("name", name)],
    );
    write_open_tag(writer, "p:cNvSpPr", &[]);
    write_empty_tag(writer, "a:spLocks", &[("noGrp", "1")]);
    write_close_tag(writer, "p:cNvSpPr");
    write_open_tag(writer, "p:nvPr", &[]);
    write_empty_tag(
        writer,
        "p:ph",
        &[("type", ph_type), ("idx", &idx.to_string())],
    );
    write_close_tag(writer, "p:nvPr");
    write_close_tag(writer, "p:nvSpPr");
    write_empty_tag(writer, "p:spPr", &[]);
    write_close_tag(writer, "p:sp");
}

fn write_notes_body_placeholder(writer: &mut Writer<Cursor<Vec<u8>>>) {
    write_open_tag(writer, "p:sp", &[]);
    write_open_tag(writer, "p:nvSpPr", &[]);
    write_empty_tag(
        writer,
        "p:cNvPr",
        &[("id", "3"), ("name", "Notes Placeholder 2")],
    );
    write_open_tag(writer, "p:cNvSpPr", &[]);
    write_empty_tag(writer, "a:spLocks", &[("noGrp", "1")]);
    write_close_tag(writer, "p:cNvSpPr");
    write_open_tag(writer, "p:nvPr", &[]);
    write_empty_tag(
        writer,
        "p:ph",
        &[("type", "body"), ("idx", "3"), ("sz", "quarter")],
    );
    write_close_tag(writer, "p:nvPr");
    write_close_tag(writer, "p:nvSpPr");
    write_empty_tag(writer, "p:spPr", &[]);
    write_open_tag(writer, "p:txBody", &[]);
    write_empty_tag(writer, "a:bodyPr", &[]);
    write_empty_tag(writer, "a:lstStyle", &[]);
    write_open_tag(writer, "a:p", &[]);
    write_open_tag(writer, "a:r", &[]);
    write_open_tag(writer, "a:t", &[]);
    write_close_tag(writer, "a:t");
    write_close_tag(writer, "a:r");
    write_close_tag(writer, "a:p");
    write_close_tag(writer, "p:txBody");
    write_close_tag(writer, "p:sp");
}

fn write_open_tag_full(writer: &mut Writer<Cursor<Vec<u8>>>, elem: &quick_xml::events::BytesStart) {
    use quick_xml::events::Event;
    writer.write_event(Event::Start(elem.clone())).unwrap();
}
