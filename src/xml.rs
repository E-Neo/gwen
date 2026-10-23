//! A tiny namespace-agnostic XML tree, used to read pptxgenjs's own
//! `theme1.xml` / `slideMaster1.xml` / `slideLayout1.xml` defaults and, in
//! tests, the resolved `slideN.xml` parts.
//!
//! The documents involved are small, so a straightforward DOM built on
//! `quick-xml` keeps namespace handling (and querying by local name) simple.

use quick_xml::Reader;
use quick_xml::events::Event;

/// One XML element. Names lose their namespace prefix; attributes keep their
/// local names and unescaped values.
#[derive(Debug, Clone, Default)]
pub struct Node {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub text: String,
    pub children: Vec<Node>,
}

impl Node {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn attr_f64(&self, name: &str) -> Option<f64> {
        self.attr(name).and_then(|v| v.trim().parse().ok())
    }

    pub fn attr_i64(&self, name: &str) -> Option<i64> {
        self.attr(name).and_then(|v| v.trim().parse().ok())
    }

    /// The first direct child with this local name.
    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    /// All direct children with this local name.
    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }

    /// The first descendant (depth first) with this local name.
    pub fn find(&self, name: &str) -> Option<&Node> {
        for child in &self.children {
            if child.name == name {
                return Some(child);
            }
            if let Some(found) = child.find(name) {
                return Some(found);
            }
        }
        None
    }

    /// Every descendant (depth first) with this local name.
    pub fn find_all<'a>(&'a self, name: &'a str, out: &mut Vec<&'a Node>) {
        for child in &self.children {
            if child.name == name {
                out.push(child);
            }
            child.find_all(name, out);
        }
    }

    /// A child's text, trimmed.
    pub fn child_text(&self, name: &str) -> Option<&str> {
        self.child(name)
            .map(|n| n.text.trim())
            .filter(|s| !s.is_empty())
    }

    /// Descendant text, trimmed (for `<a:t>` payloads).
    pub fn find_text(&self, name: &str) -> Option<&str> {
        self.find(name)
            .map(|n| n.text.trim())
            .filter(|s| !s.is_empty())
    }
}

fn local(name: &str) -> String {
    match name.rsplit_once(':') {
        Some((_, local)) => local.to_string(),
        None => name.to_string(),
    }
}

fn element(e: &quick_xml::events::BytesStart<'_>) -> Node {
    let mut node = Node {
        name: local(e.name().as_ref()),
        ..Node::default()
    };
    for attr in e.attributes().flatten() {
        let key = local(attr.key.as_ref());
        let value = attr
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .unwrap_or_default()
            .into_owned();
        node.attrs.push((key, value));
    }
    node
}

/// Parse an XML document into a tree rooted at its document element.
pub fn parse(source: &str) -> Result<Node, String> {
    let mut reader = Reader::from_str(source);
    reader.config_mut().trim_text(false);
    let mut stack: Vec<Node> = Vec::new();
    let mut root: Option<Node> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => stack.push(element(&e)),
            Ok(Event::Empty(e)) => {
                let node = element(&e);
                attach(&mut stack, &mut root, node);
            }
            Ok(Event::End(_)) => {
                if let Some(node) = stack.pop() {
                    attach(&mut stack, &mut root, node);
                }
            }
            Ok(Event::Text(e)) => {
                if let Some(parent) = stack.last_mut() {
                    parent
                        .text
                        .push_str(&e.xml_content(quick_xml::XmlVersion::Implicit1_0));
                }
            }
            Ok(Event::GeneralRef(e)) => {
                if let Some(parent) = stack.last_mut() {
                    let decoded = match e.as_ref() {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        name => {
                            // quick-xml only reports unknown names here; numeric
                            // references are decoded inline.
                            parent.text.push_str(&format!("&{name};"));
                            continue;
                        }
                    };
                    parent.text.push_str(decoded);
                }
            }
            Ok(Event::CData(e)) => {
                if let Some(parent) = stack.last_mut() {
                    parent.text.push_str(e.as_ref());
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => return Err(format!("invalid XML: {e}")),
        }
    }
    root.ok_or_else(|| "the XML document has no root element".to_string())
}

fn attach(stack: &mut [Node], root: &mut Option<Node>, node: Node) {
    match stack.last_mut() {
        Some(parent) => parent.children.push(node),
        None => {
            if root.is_none() {
                *root = Some(node);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_namespaces_attrs_and_text() {
        let xml = r#"<p:root xmlns:p="x"><p:sp name="A"><a:t>hi &amp; bye</a:t></p:sp><p:sp name="B"/></p:root>"#;
        let root = parse(xml).unwrap();
        assert_eq!(root.name, "root");
        let a = root.child("sp").unwrap();
        assert_eq!(a.attr("name"), Some("A"));
        assert_eq!(a.find_text("t"), Some("hi & bye"));
        assert_eq!(root.children_named("sp").count(), 2);
        let mut found = Vec::new();
        root.find_all("t", &mut found);
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn empty_and_repeated_elements() {
        let root = parse("<r><a x=\"1\"/><a x=\"2\"/></r>").unwrap();
        let all: Vec<&Node> = root.children_named("a").collect();
        assert_eq!(all.len(), 2);
        assert_eq!(all[1].attr("x"), Some("2"));
    }

    #[test]
    fn reports_malformed_input() {
        assert!(parse("<r><a></r>").is_err());
        assert!(parse("").is_err());
    }
}
