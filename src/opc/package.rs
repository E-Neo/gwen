use std::collections::HashMap;
use std::io::{Cursor, Read, Seek, Write};
use std::path::Path;

use quick_xml::Reader;
use quick_xml::Writer;
use quick_xml::events::{BytesDecl, Event};
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
        Self::from_archive(&mut archive)
    }

    /// Build a package directly from in-memory parts and relationships.
    #[cfg(test)]
    pub fn from_parts(
        parts: HashMap<String, Vec<u8>>,
        relationships: HashMap<String, HashMap<String, Relationship>>,
    ) -> Self {
        Package {
            parts,
            relationships,
        }
    }

    /// Resolve a relationship's target to the URI of the part it points at,
    /// relative to the source part. Returns `None` for external targets.
    pub fn resolve_relationship_target(
        &self,
        source_uri: &str,
        rel: &Relationship,
    ) -> Option<String> {
        if rel.target_mode.as_deref() == Some("External") {
            return None;
        }
        let target = if rel.target.starts_with('/') {
            rel.target.trim_start_matches('/').to_string()
        } else {
            let base_dir = source_uri
                .rsplit_once('/')
                .map(|(d, _)| d.to_string())
                .unwrap_or_default();
            let mut segments: Vec<&str> = if base_dir.is_empty() {
                Vec::new()
            } else {
                base_dir.split('/').collect()
            };
            for seg in rel.target.split('/') {
                match seg {
                    "" | "." => {}
                    ".." => {
                        segments.pop();
                    }
                    _ => segments.push(seg),
                }
            }
            segments.join("/")
        };
        self.parts.contains_key(&target).then_some(target)
    }

    fn from_archive<R: Read + Seek>(archive: &mut ZipArchive<R>) -> AppResult<Self> {
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

    pub fn remove_part(&mut self, uri: &str) {
        self.parts.remove(uri);
    }

    pub fn remove_relationship(&mut self, source_uri: &str, r_id: &str) {
        if let Some(rels) = self.relationships.get_mut(source_uri) {
            rels.remove(r_id);
        }
    }

    pub fn remove_all_relationships(&mut self, source_uri: &str) {
        self.relationships.remove(source_uri);
    }

    pub fn remove_content_type_override(&mut self, part_name: &str) -> AppResult<()> {
        let raw = self
            .get_part("[Content_Types].xml")
            .ok_or_else(|| AppError::PartNotFound("[Content_Types].xml".to_string()))?
            .to_vec();

        let mut reader = Reader::from_reader(&raw[..]);
        reader.config_mut().trim_text(true);
        let mut writer = Writer::new(Vec::new());
        let mut inside_types = false;
        let mut buf = Vec::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    if e.name().as_ref() == b"Types" {
                        inside_types = true;
                    }
                    writer
                        .write_event(Event::Start(e.clone()))
                        .map_err(AppError::Io)?;
                }
                Ok(Event::Empty(ref e)) => {
                    if inside_types && e.name().as_ref() == b"Override" {
                        let skip = e.attributes().flatten().any(|a| {
                            a.key.as_ref() == b"PartName"
                                && a.value.as_ref() == part_name.as_bytes()
                        });
                        if skip {
                            continue;
                        }
                    }
                    writer
                        .write_event(Event::Empty(e.clone()))
                        .map_err(AppError::Io)?;
                }
                Ok(Event::End(ref e)) => {
                    if e.name().as_ref() == b"Types" {
                        inside_types = false;
                    }
                    writer
                        .write_event(Event::End(e.clone()))
                        .map_err(AppError::Io)?;
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(AppError::Xml(e)),
                Ok(e) => {
                    writer.write_event(e).map_err(AppError::Io)?;
                }
            }
        }

        self.set_part("[Content_Types].xml", writer.into_inner());
        Ok(())
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

    pub fn get_next_notes_num(&self) -> u32 {
        let mut max_num = 0u32;
        for key in self.parts.keys() {
            if let Some(rest) = key.strip_prefix("ppt/notesSlides/notesSlide")
                && let Some(num_str) = rest.strip_suffix(".xml")
                && let Ok(n) = num_str.parse::<u32>()
                && n > max_num
            {
                max_num = n;
            }
        }
        max_num + 1
    }

    pub fn add_content_type_override(
        &mut self,
        part_name: &str,
        content_type: &str,
    ) -> AppResult<()> {
        let raw = self
            .get_part("[Content_Types].xml")
            .ok_or_else(|| AppError::PartNotFound("[Content_Types].xml".to_string()))?
            .to_vec();

        let mut reader = Reader::from_reader(&raw[..]);
        reader.config_mut().trim_text(true);
        let mut writer = Writer::new(Vec::new());
        let mut inserted = false;
        let mut buf = Vec::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::End(ref e)) if e.name().as_ref() == b"Types" && !inserted => {
                    let mut override_elem = quick_xml::events::BytesStart::new("Override");
                    override_elem.push_attribute(("PartName", part_name));
                    override_elem.push_attribute(("ContentType", content_type));
                    writer
                        .write_event(Event::Empty(override_elem))
                        .map_err(AppError::Io)?;
                    inserted = true;
                    writer
                        .write_event(Event::End(e.clone()))
                        .map_err(AppError::Io)?;
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(AppError::Xml(e)),
                Ok(e) => {
                    writer.write_event(e).map_err(AppError::Io)?;
                }
            }
        }

        if !inserted {
            let mut bytes = raw;
            let insert = format!(
                "  <Override PartName=\"{}\" ContentType=\"{}\"/>\n",
                part_name, content_type
            );
            let close_tag = "</Types>";
            if let Some(pos) = bytes
                .windows(close_tag.len())
                .position(|w| w == close_tag.as_bytes())
            {
                bytes.splice(pos..pos, insert.into_bytes());
            }
            self.set_part("[Content_Types].xml", bytes);
        } else {
            self.set_part("[Content_Types].xml", writer.into_inner());
        }
        Ok(())
    }
}

pub(crate) fn rels_source_key(name: &str) -> String {
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

pub(crate) fn parse_rels_xml(data: &[u8]) -> AppResult<HashMap<String, Relationship>> {
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
        .write_event(Event::Decl(BytesDecl::new(
            "1.0",
            Some("UTF-8"),
            Some("yes"),
        )))
        .map_err(AppError::Io)?;
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
