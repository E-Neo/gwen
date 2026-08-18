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

/// A human-readable location for an edit path, built from the parsed markdown
/// document so shape names come from the mirror. Examples:
///
/// - `slides[4].notes.shapes[0].text_frame` →
///   `Slide 5, notes, shape 1 ("Slide Image Placeholder 1"), text frame`
/// - `slides[0].shapes[1].table.rows[0].cells[0].text_frame.paragraphs[1]` →
///   `Slide 1, shape 2 ("Table 2"), table, row 1, cell 1, paragraph 2`
pub fn describe_path(doc: &Value, path: &str) -> String {
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
                    let shape = cur.and_then(|c| c.get("shapes")).and_then(|s| s.get(idx));
                    let name = shape.and_then(|s| s.get("name")).and_then(Value::as_str);
                    let ty = shape
                        .and_then(|s| s.get("shape_type"))
                        .and_then(Value::as_str)
                        .map(|t| t.to_ascii_lowercase());
                    let label = match (name, ty) {
                        (Some(n), _) if !n.is_empty() => format!("shape {} ({n})", idx + 1),
                        (_, Some(t)) => format!("shape {} ({t})", idx + 1),
                        _ => format!("shape {}", idx + 1),
                    };
                    crumbs.push(label);
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
            "paragraphs" | "runs" | "colors" | "fonts" => {}
            _ => crumbs.push(seg.to_string()),
        }
        cur = cur.and_then(|c| c.get(seg));
        i += 1;
    }
    crumbs.join(", ")
}

/// A next-step suggestion for a failed edit, keyed on the error message.
pub fn advice(message: &str) -> Option<String> {
    if message.contains("no text frame") {
        return Some(
            "The shape has no text body; remove its paragraph block (or the whole shape marker) instead of the frame.".into(),
        );
    }
    if message.contains("no table") {
        return Some(
            "The shape is not a table; table edits only apply to `type=\"table\"` shapes.".into(),
        );
    }
    if message.contains("has no notes") {
        return Some("Add a `### Notes` heading under the slide to create notes for it.".into());
    }
    if message.contains("already has notes") {
        return Some("Delete the slide's `### Notes` section to remove its notes.".into());
    }
    if message.contains("derived from the deck and cannot be edited") {
        return Some("Remove this field from the mirror; it is derived from the deck and cannot be changed here.".into());
    }
    if message.contains("cannot be deleted") || message.contains("cannot be removed") {
        return Some("Set the attribute to a value instead of removing it.".into());
    }
    if message.contains("Grouped shapes are read-only") {
        return Some(
            "Edit grouped shapes in the original deck, or ungroup the shape first.".into(),
        );
    }
    if message.contains("edited as a whole") {
        return Some(
            "Edit the individual style properties under `default_paragraph_style` instead.".into(),
        );
    }
    if message.contains("chart") || message.contains("Chart") {
        return Some(
            "Chart data lives in the embedded workbook; change it in Excel, not in the mirror."
                .into(),
        );
    }
    if message.contains("theme")
        || message.contains("color scheme")
        || message.contains("font scheme")
    {
        return Some(
            "Edit the deck's theme part (theme/theme1.xml) to change theme colors and fonts."
                .into(),
        );
    }
    if message.contains("no text body") || message.contains("no txBody") {
        return Some("The cell is empty; write some text into it first.".into());
    }
    None
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{advice, describe_path};

    #[test]
    fn describe_path_reads_shape_names() {
        let doc = json!({
            "slides": [
                { "shapes": [ { "name": "TextBox 1", "shape_type": "TEXTBOX" } ] },
                {
                    "notes": {
                        "shapes": [ { "name": "Slide Image Placeholder 1", "shape_type": "PLACEHOLDER" } ]
                    }
                }
            ]
        });
        assert_eq!(
            describe_path(&doc, "slides[0].shapes[0].text_frame.paragraphs[0]"),
            "Slide 1, shape 1 (TextBox 1), text frame, paragraph 1"
        );
        assert_eq!(
            describe_path(&doc, "slides[1].notes.shapes[0].text_frame"),
            "Slide 2, notes, shape 1 (Slide Image Placeholder 1), text frame"
        );
    }

    #[test]
    fn describe_path_falls_back_to_shape_type() {
        let doc = json!({ "slides": [ { "shapes": [ { "shape_type": "TABLE" } ] } ] });
        assert_eq!(
            describe_path(&doc, "slides[0].shapes[0].table.rows[0].cells[0]"),
            "Slide 1, shape 1 (table), table, row 1, cell 1"
        );
    }

    #[test]
    fn advice_suggests_next_steps() {
        assert!(
            advice("The shape has no text frame")
                .unwrap()
                .contains("text body")
        );
        assert!(
            advice("`name` is derived from the deck and cannot be edited in the mirror")
                .unwrap()
                .contains("Remove this field")
        );
        assert!(
            advice("`left` cannot be removed from a shape")
                .unwrap()
                .contains("value")
        );
        assert!(
            advice("the theme has no color scheme")
                .unwrap()
                .contains("theme part")
        );
        assert!(advice("something unrelated").is_none());
    }
}
