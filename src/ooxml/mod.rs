//! A tiny mutable XML tree and serializer. Every generated part is built from
//! this, so nesting and escaping are always correct — no string splicing.

pub mod content_types;
pub mod package;
pub mod rels;

use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};

/// An XML element: a name, attributes, and either children or text.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Elem {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Elem>,
    pub text: Option<String>,
}

impl Elem {
    /// A new empty element. Attributes and children can be chained.
    pub fn new(name: impl Into<String>) -> Self {
        Elem {
            name: name.into(),
            ..Default::default()
        }
    }

    pub fn attr(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attrs.push((key.into(), value.into()));
        self
    }

    pub fn child(mut self, child: Elem) -> Self {
        self.children.push(child);
        self
    }

    pub fn children(mut self, children: impl IntoIterator<Item = Elem>) -> Self {
        self.children.extend(children);
        self
    }

    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    fn write(&self, writer: &mut Writer<Vec<u8>>) {
        let mut start = BytesStart::new(self.name.as_str());
        for (key, value) in &self.attrs {
            start.push_attribute((key.as_str(), value.as_str()));
        }
        if self.children.is_empty() && self.text.is_none() {
            writer.write_event(Event::Empty(start)).ok();
            return;
        }
        writer.write_event(Event::Start(start)).ok();
        if let Some(text) = &self.text {
            writer.write_event(Event::Text(BytesText::new(text))).ok();
        }
        for child in &self.children {
            child.write(writer);
        }
        writer
            .write_event(Event::End(BytesEnd::new(self.name.as_str())))
            .ok();
    }

    /// Serialize to a standalone XML document (with declaration).
    pub fn to_xml(&self) -> Vec<u8> {
        let mut writer = Writer::new(Vec::new());
        writer
            .write_event(Event::Decl(BytesDecl::new(
                "1.0",
                Some("UTF-8"),
                Some("yes"),
            )))
            .ok();
        self.write(&mut writer);
        writer.into_inner()
    }
}
