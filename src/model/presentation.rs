use quick_xml::Reader;
use quick_xml::events::Event;

use crate::error::{AppError, AppResult};

#[derive(serde::Serialize)]
pub struct Presentation {
    pub slide_uris: Vec<String>,
    pub slide_width: i64,
    pub slide_height: i64,
}

impl Presentation {
    pub fn parse(data: &[u8]) -> AppResult<Self> {
        let mut reader = Reader::from_reader(data);
        reader.config_mut().trim_text(true);
        let mut slide_uris = Vec::new();
        let mut slide_width = 0i64;
        let mut slide_height = 0i64;
        let mut buf = Vec::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e)) => match e.name().as_ref() {
                    b"p:sldId" => {
                        let mut r_id = String::new();
                        for attr in e.attributes() {
                            if let Ok(a) = attr
                                && a.key.as_ref() == b"r:id"
                            {
                                r_id = String::from_utf8_lossy(&a.value).to_string();
                            }
                        }
                        if !r_id.is_empty() {
                            slide_uris.push(r_id);
                        }
                    }
                    b"p:sldSz" => {
                        for a in e.attributes().flatten() {
                            match a.key.as_ref() {
                                b"cx" => {
                                    slide_width =
                                        String::from_utf8_lossy(&a.value).parse().unwrap_or(0);
                                }
                                b"cy" => {
                                    slide_height =
                                        String::from_utf8_lossy(&a.value).parse().unwrap_or(0);
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                },
                Ok(Event::Eof) => break,
                Err(e) => return Err(AppError::Xml(e)),
                _ => {}
            }
        }

        Ok(Presentation {
            slide_uris,
            slide_width,
            slide_height,
        })
    }

    pub fn resolve_slide_uris(
        &self,
        rels: &std::collections::HashMap<String, super::super::opc::Relationship>,
    ) -> Vec<String> {
        self.slide_uris
            .iter()
            .map(|r_id| {
                rels.get(r_id)
                    .map(|r| {
                        let target = &r.target;
                        if target.starts_with("ppt/") || target.starts_with('/') {
                            target.clone()
                        } else {
                            format!("ppt/{target}")
                        }
                    })
                    .unwrap_or_default()
            })
            .collect()
    }
}
