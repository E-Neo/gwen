//! An in-memory OPC package: parts, relationships, and zip serialization.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use crate::error::Result;
use crate::ooxml::content_types;
use crate::ooxml::rels::{self, Relationship};

#[derive(Debug, Default)]
pub struct Package {
    parts: BTreeMap<String, Vec<u8>>,
    rels: BTreeMap<String, Vec<Relationship>>,
}

impl Package {
    pub fn new() -> Self {
        Package::default()
    }

    pub fn add_part(&mut self, uri: impl Into<String>, data: Vec<u8>) {
        self.parts.insert(uri.into(), data);
    }

    pub fn part(&self, uri: &str) -> Option<&[u8]> {
        self.parts.get(uri).map(Vec::as_slice)
    }

    pub fn part_uris(&self) -> impl Iterator<Item = &String> {
        self.parts.keys()
    }

    pub fn relationships(&self, source: &str) -> Option<&[Relationship]> {
        self.rels.get(source).map(Vec::as_slice)
    }

    pub fn rel_sources(&self) -> impl Iterator<Item = &String> {
        self.rels.keys()
    }

    /// Add an internal relationship from `source` to `target` (both package
    /// URIs). The stored target is always the relative path. Returns the id.
    pub fn relate(&mut self, source: &str, rel_type: &str, target: &str) -> String {
        let id = self.next_rel_id(source);
        self.rels
            .entry(source.to_string())
            .or_default()
            .push(Relationship {
                id: id.clone(),
                rel_type: rel_type.to_string(),
                target: rels::relative_target(source, target),
                external: false,
            });
        id
    }

    /// Add an external relationship (e.g. a hyperlink URL).
    pub fn relate_external(&mut self, source: &str, rel_type: &str, target: &str) -> String {
        let id = self.next_rel_id(source);
        self.rels
            .entry(source.to_string())
            .or_default()
            .push(Relationship {
                id: id.clone(),
                rel_type: rel_type.to_string(),
                target: target.to_string(),
                external: true,
            });
        id
    }

    fn next_rel_id(&self, source: &str) -> String {
        let max = self
            .rels
            .get(source)
            .into_iter()
            .flatten()
            .filter_map(|r| r.id.strip_prefix("rId").and_then(|n| n.parse::<u32>().ok()))
            .max()
            .unwrap_or(0);
        format!("rId{}", max + 1)
    }

    /// Write the package to `path`, generating `[Content_Types].xml` and every
    /// `.rels` part.
    pub fn save(mut self, path: &Path) -> Result<()> {
        let uris: Vec<String> = self.parts.keys().cloned().collect();
        let content_types = content_types::content_types_xml(&uris)?;
        self.parts
            .insert("[Content_Types].xml".into(), content_types);

        let rels_sources: Vec<String> = self.rels.keys().cloned().collect();
        for source in &rels_sources {
            let rels = &self.rels[source];
            let name = rels::rels_part_name(source);
            self.parts.insert(name, rels::rels_xml(rels));
        }

        let file = std::fs::File::create(path)
            .map_err(|e| miette::miette!("cannot create {}: {e}", path.display()))?;
        let mut zip = zip::ZipWriter::new(file);
        let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(zip::DateTime::default());
        for (uri, data) in &self.parts {
            zip.start_file(uri, options)
                .map_err(|e| miette::miette!("cannot write `{uri}`: {e}"))?;
            zip.write_all(data)
                .map_err(|e| miette::miette!("cannot write `{uri}`: {e}"))?;
        }
        zip.finish()
            .map_err(|e| miette::miette!("cannot finish zip: {e}"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relationships_are_numbered_and_relative() {
        let mut pkg = Package::new();
        pkg.add_part("ppt/presentation.xml", b"<p:presentation/>".to_vec());
        pkg.add_part("ppt/slides/slide1.xml", b"<p:sld/>".to_vec());
        let id = pkg.relate("ppt/presentation.xml", rels::SLIDE, "ppt/slides/slide1.xml");
        assert_eq!(id, "rId1");
        let rels = pkg.relationships("ppt/presentation.xml").unwrap();
        assert_eq!(rels[0].target, "slides/slide1.xml");
        assert!(!rels[0].external);
    }
}
