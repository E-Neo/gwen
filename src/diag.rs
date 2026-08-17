use std::fmt;

use gwen_md::MdSpan;
use miette::{Diagnostic, LabeledSpan, NamedSource, SourceCode, SourceSpan};
use serde_json::Value;

/// A miette diagnostic. With `src`/`span` set it points into the markdown
/// mirror (rendered as an annotated code region); otherwise it is a plain
/// message.
#[derive(Debug)]
pub struct Diag {
    message: String,
    src: Option<NamedSource<String>>,
    span: Option<SourceSpan>,
    help: Option<String>,
}

impl Diag {
    pub fn plain(message: impl Into<String>) -> Self {
        Diag {
            message: message.into(),
            src: None,
            span: None,
            help: None,
        }
    }

    /// A diagnostic anchored at `span` in the markdown `source`.
    pub fn at(
        source: &str,
        message: impl Into<String>,
        span: &MdSpan,
        help: Option<String>,
    ) -> Self {
        Diag {
            message: message.into(),
            src: Some(NamedSource::new("deck.md", source.to_string())),
            span: Some(SourceSpan::new(span.offset.into(), span.len)),
            help,
        }
    }
}

impl fmt::Display for Diag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for Diag {}

impl Diagnostic for Diag {
    fn code<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        None
    }

    fn severity(&self) -> Option<miette::Severity> {
        None
    }

    fn help<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        self.help
            .as_deref()
            .map(|s| Box::new(s.to_string()) as Box<dyn fmt::Display>)
    }

    fn url<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        None
    }

    fn source_code(&self) -> Option<&dyn SourceCode> {
        self.src.as_ref().map(|s| s as &dyn SourceCode)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        self.span.map(|sp| {
            Box::new(std::iter::once(LabeledSpan::new(
                Some("here".to_string()),
                sp.offset(),
                sp.len(),
            ))) as Box<dyn Iterator<Item = LabeledSpan>>
        })
    }

    fn related(&self) -> Option<Box<dyn Iterator<Item = &dyn Diagnostic> + '_>> {
        None
    }

    fn diagnostic_source(&self) -> Option<&dyn Diagnostic> {
        None
    }
}

/// Wrap a gwen-pptx (or other displayable) error into a plain miette report
/// with no markdown location.
pub fn plain<E: fmt::Display + Send + Sync + 'static>(e: E) -> miette::Report {
    miette::Report::new(Diag::plain(e.to_string()))
}

/// A human-readable breadcrumb for an edit path, e.g.
/// `slides[3].shapes[1].text_frame.paragraphs[0].runs` becomes
/// `Slide 4. shape 2 (table). text frame. paragraph 1. runs`.
pub fn markdown_path_breadcrumb(doc: &Value, path: &str) -> String {
    let segs: Vec<&str> = path.split('.').collect();
    let mut crumbs: Vec<String> = Vec::new();
    let mut cur: Option<&Value> = Some(doc);
    let mut i = 0;
    while i < segs.len() {
        let seg = segs[i];
        if let Some(open) = seg.find('[') {
            let field = &seg[..open];
            let idx: usize = seg[open + 1..seg.len().saturating_sub(1)]
                .parse()
                .unwrap_or(0);
            match field {
                "slides" => crumbs.push(format!("Slide {}", idx + 1)),
                "slide_masters" => crumbs.push(format!("Master {}", idx + 1)),
                "shapes" => {
                    let ty = cur
                        .and_then(|c| c.get("shapes"))
                        .and_then(|s| s.get(idx))
                        .and_then(|s| s.get("shape_type"))
                        .and_then(Value::as_str);
                    match ty {
                        Some(t) => {
                            crumbs.push(format!("shape {} ({})", idx + 1, t.to_ascii_lowercase()))
                        }
                        None => crumbs.push(format!("shape {}", idx + 1)),
                    }
                }
                "rows" => crumbs.push(format!("row {}", idx + 1)),
                "cells" => crumbs.push(format!("cell {}", idx + 1)),
                "paragraphs" => crumbs.push(format!("paragraph {}", idx + 1)),
                "runs" => crumbs.push(format!("run {}", idx + 1)),
                "grid" => crumbs.push(format!("column {}", idx + 1)),
                _ => {}
            }
            cur = cur.and_then(|c| c.get(field)).and_then(|v| v.get(idx));
            i += 1;
            continue;
        }
        match seg {
            "text_frame" => crumbs.push("text frame".into()),
            "notes" => crumbs.push("notes".into()),
            "table" => crumbs.push("table".into()),
            "default_paragraph_style" => crumbs.push("default paragraph style".into()),
            _ => {}
        }
        cur = cur.and_then(|c| c.get(seg));
        i += 1;
    }
    crumbs.join(". ")
}
