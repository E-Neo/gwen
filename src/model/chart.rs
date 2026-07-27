use quick_xml::Reader;
use quick_xml::events::Event;

use crate::dto::{ChartDto, ChartSeriesDto};
use crate::error::{AppError, AppResult};

pub fn parse_chart_xml(data: &[u8]) -> AppResult<ChartDto> {
    let mut reader = Reader::from_reader(data);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();

    let mut chart_type: Option<String> = None;
    let mut series: Vec<ChartSeriesDto> = Vec::new();
    let mut in_plot_area = false;
    let mut in_ser = false;
    let mut in_cat = false;
    let mut in_val = false;
    let mut in_str_lit = false;
    let mut in_num_lit = false;
    let mut in_pt = false;
    let mut in_v = false;
    let mut in_str_ref = false;
    let mut in_num_ref = false;
    let mut ser_name: Option<String> = None;
    let mut categories: Vec<String> = Vec::new();
    let mut values: Vec<f64> = Vec::new();
    let mut text_buf = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let en = e.name();
                let enb = en.as_ref();
                match enb {
                    b"c:plotArea" => in_plot_area = true,
                    b"c:barChart" | b"c:lineChart" | b"c:pieChart" | b"c:scatterChart"
                    | b"c:doughnutChart" | b"c:radarChart" | b"c:areaChart" => {
                        if in_plot_area && chart_type.is_none() {
                            chart_type = Some(
                                String::from_utf8_lossy(enb)
                                    .strip_prefix("c:")
                                    .unwrap_or("")
                                    .to_string(),
                            );
                        }
                    }
                    b"c:ser" => {
                        in_ser = true;
                        ser_name = None;
                    }
                    b"c:tx" if in_ser => {}
                    b"c:strRef" if in_ser => in_str_ref = true,
                    b"c:numRef" if in_ser => in_num_ref = true,
                    b"c:cat" if in_ser => {
                        in_cat = true;
                        categories.clear();
                    }
                    b"c:val" if in_ser => {
                        in_val = true;
                        values.clear();
                    }
                    b"c:strLit" if in_cat || in_ser => in_str_lit = true,
                    b"c:numLit" if in_val || in_ser => in_num_lit = true,
                    b"c:pt" if in_str_lit || in_num_lit => {
                        in_pt = true;
                    }
                    b"c:v" if in_pt => {
                        in_v = true;
                        text_buf.clear();
                    }
                    b"c:f" if in_str_ref || in_num_ref => {
                        // skip formula references
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) => {
                if in_v && let Ok(t) = String::from_utf8(e.to_vec()) {
                    text_buf.push_str(&t);
                }
            }
            Ok(Event::Empty(ref _e)) => {}
            Ok(Event::End(ref e)) => {
                let en = e.name();
                let enb = en.as_ref();
                match enb {
                    b"c:plotArea" => in_plot_area = false,
                    b"c:ser" => {
                        series.push(ChartSeriesDto {
                            name: ser_name.take(),
                            categories: std::mem::take(&mut categories),
                            values: std::mem::take(&mut values),
                        });
                        in_ser = false;
                        in_cat = false;
                        in_val = false;
                    }
                    b"c:strLit" => in_str_lit = false,
                    b"c:numLit" => in_num_lit = false,
                    b"c:pt" => {
                        if (in_str_lit || in_num_lit) && !text_buf.is_empty() {
                            if in_cat {
                                categories.push(std::mem::take(&mut text_buf));
                            } else if in_ser && !in_val && !in_cat {
                                ser_name = Some(std::mem::take(&mut text_buf));
                            }
                        }
                        in_pt = false;
                        in_v = false;
                    }
                    b"c:ptCount" => {}
                    b"c:v" => {
                        if in_pt && in_num_lit && in_val {
                            if let Ok(v) = text_buf.trim().parse::<f64>() {
                                values.push(v);
                            } else {
                                values.push(0.0);
                            }
                        } else if in_pt && in_str_lit && in_cat {
                            categories.push(std::mem::take(&mut text_buf));
                        } else if in_ser && !in_cat && !in_val {
                            ser_name = Some(std::mem::take(&mut text_buf));
                        }
                        text_buf.clear();
                        in_v = false;
                    }
                    b"c:strRef" => in_str_ref = false,
                    b"c:numRef" => in_num_ref = false,
                    b"c:cat" => in_cat = false,
                    b"c:val" => in_val = false,
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(AppError::Xml(e)),
            _ => {}
        }
    }

    Ok(ChartDto {
        chart_type,
        r_id: None,
        series,
    })
}
