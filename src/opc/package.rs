use std::collections::HashMap;
use std::io::{Cursor, Read, Write};
use std::path::Path;

use quick_xml::Reader;
use quick_xml::Writer;
use quick_xml::events::Event;
use zip::CompressionMethod;
use zip::read::ZipArchive;
use zip::write::FileOptions;

use super::relationship::Relationship;
use crate::error::{AppError, AppResult};

pub struct Package {
    parts: HashMap<String, Vec<u8>>,
    relationships: HashMap<String, HashMap<String, Relationship>>,
}

impl Package {
    pub fn open(path: &Path) -> AppResult<Self> {
        let file = std::fs::File::open(path)?;
        let mut archive = ZipArchive::new(file)?;
        let mut parts = HashMap::new();
        let mut relationships = HashMap::new();

        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            let name = entry.name().to_string();
            let mut data = Vec::new();
            entry.read_to_end(&mut data)?;

            if name.ends_with(".rels") || name.contains("_rels/") {
                let rels = parse_rels_xml(&data)?;
                let source_uri = rels_source_key(&name);
                relationships.insert(source_uri, rels);
            } else {
                parts.insert(name, data);
            }
        }

        Ok(Package {
            parts,
            relationships,
        })
    }

    pub fn get_part(&self, uri: &str) -> Option<&[u8]> {
        self.parts.get(uri).map(|v| v.as_slice())
    }

    pub fn set_part(&mut self, uri: &str, data: Vec<u8>) {
        self.parts.insert(uri.to_string(), data);
    }

    pub fn get_rels(&self, uri: &str) -> Option<&HashMap<String, Relationship>> {
        self.relationships.get(uri)
    }

    #[allow(dead_code)]
    pub fn get_rels_mut(&mut self, uri: &str) -> Option<&mut HashMap<String, Relationship>> {
        self.relationships.get_mut(uri)
    }

    #[allow(dead_code)]
    pub fn add_relationship(&mut self, source_uri: &str, rel: Relationship) -> String {
        let rels = self
            .relationships
            .entry(source_uri.to_string())
            .or_default();
        let max_id = rels
            .keys()
            .filter_map(|k| k.strip_prefix("rId").and_then(|s| s.parse::<u32>().ok()))
            .max()
            .unwrap_or(0);
        let new_id = format!("rId{}", max_id + 1);
        let mut rel = rel;
        rel.id = new_id.clone();
        rels.insert(new_id.clone(), rel);
        new_id
    }

    pub fn save(&self, path: &Path) -> AppResult<()> {
        let file = std::fs::File::create(path)?;
        let mut zip = zip::ZipWriter::new(file);
        let options: FileOptions<'_, ()> =
            FileOptions::default().compression_method(CompressionMethod::Deflated);

        let mut sorted_parts: Vec<&String> = self.parts.keys().collect();
        sorted_parts.sort();
        for name in sorted_parts {
            let data = self.parts.get(name).unwrap();
            zip.start_file(name, options)?;
            zip.write_all(data)?;
        }

        let rels_uri_map = build_rels_uri_map(&self.relationships);
        let mut sorted_rels: Vec<&String> = rels_uri_map.keys().collect();
        sorted_rels.sort();
        for name in sorted_rels {
            let source_uri = rels_uri_map.get(name).unwrap();
            let rels = self.relationships.get(source_uri).unwrap();
            let xml = serialize_rels_xml(rels)?;
            zip.start_file(name, options)?;
            zip.write_all(&xml)?;
        }

        zip.finish()?;
        Ok(())
    }
}

fn rels_source_key(name: &str) -> String {
    if name == "_rels/.rels" {
        return String::new();
    }
    let without_suffix = name.strip_suffix(".rels").unwrap_or(name);
    if let Some((base, part)) = without_suffix.split_once("/_rels/") {
        format!("{base}/{part}")
    } else {
        String::new()
    }
}

fn build_rels_uri_map(
    relationships: &HashMap<String, HashMap<String, Relationship>>,
) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for source_uri in relationships.keys() {
        let rels_name = if source_uri.is_empty() {
            "_rels/.rels".to_string()
        } else {
            let base = source_uri.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
            let file = source_uri
                .rsplit_once('/')
                .map(|(_, f)| f)
                .unwrap_or(source_uri);
            let rels_dir = if base.is_empty() {
                "_rels"
            } else {
                &format!("{}/_rels", base)
            };
            format!("{}/{}.rels", rels_dir, file)
        };
        map.insert(rels_name, source_uri.clone());
    }
    map
}

fn parse_rels_xml(data: &[u8]) -> AppResult<HashMap<String, Relationship>> {
    let mut reader = Reader::from_reader(data);
    reader.config_mut().trim_text(true);
    let mut rels = HashMap::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e)) => {
                if e.name().as_ref() == b"Relationship" {
                    let mut id = String::new();
                    let mut target = String::new();
                    let mut target_mode = None;
                    let mut rel_type = String::new();

                    for a in e.attributes().flatten() {
                        match a.key.as_ref() {
                            b"Id" => id = String::from_utf8_lossy(&a.value).to_string(),
                            b"Target" => target = String::from_utf8_lossy(&a.value).to_string(),
                            b"TargetMode" => {
                                target_mode = Some(String::from_utf8_lossy(&a.value).to_string())
                            }
                            b"Type" => rel_type = String::from_utf8_lossy(&a.value).to_string(),
                            _ => {}
                        }
                    }
                    if !id.is_empty() {
                        rels.insert(
                            id.clone(),
                            Relationship {
                                id,
                                target,
                                target_mode,
                                rel_type,
                            },
                        );
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(AppError::Xml(e)),
            _ => {}
        }
    }
    Ok(rels)
}

fn serialize_rels_xml(rels: &HashMap<String, Relationship>) -> AppResult<Vec<u8>> {
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    writer
        .write_event(Event::Start(
            quick_xml::events::BytesStart::new("Relationships").with_attributes(vec![(
                b"xmlns" as &[u8],
                b"http://schemas.openxmlformats.org/package/2006/relationships" as &[u8],
            )]),
        ))
        .map_err(AppError::Io)?;

    let mut sorted_ids: Vec<&String> = rels.keys().collect();
    sorted_ids.sort();

    for id in sorted_ids {
        let rel = rels.get(id).unwrap();
        let mut elem = quick_xml::events::BytesStart::new("Relationship");
        elem.push_attribute(("Id", rel.id.as_str()));
        elem.push_attribute(("Type", rel.rel_type.as_str()));
        elem.push_attribute(("Target", rel.target.as_str()));
        if let Some(ref mode) = rel.target_mode {
            elem.push_attribute(("TargetMode", mode.as_str()));
        }
        writer
            .write_event(Event::Empty(elem))
            .map_err(AppError::Io)?;
    }

    writer
        .write_event(Event::End(quick_xml::events::BytesEnd::new(
            "Relationships",
        )))
        .map_err(AppError::Io)?;

    let inner = writer.into_inner().into_inner();
    Ok(inner)
}
