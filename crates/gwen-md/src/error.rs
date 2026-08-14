use std::fmt;

/// A source location in the markdown document, rendered rustc-style by the
/// CLI.
#[derive(Debug, Clone)]
pub struct MdSpan {
    /// 1-based line in the original markdown.
    pub line: usize,
    /// 1-based column.
    pub col: usize,
    /// The source line, for the caret display.
    pub snippet: String,
}

/// A diagnostic attached to a location in the markdown document.
#[derive(Debug, Clone)]
pub struct MdError {
    pub message: String,
    pub span: MdSpan,
}

impl MdError {
    pub fn new(
        message: impl Into<String>,
        line: usize,
        col: usize,
        snippet: impl Into<String>,
    ) -> Self {
        MdError {
            message: message.into(),
            span: MdSpan {
                line,
                col,
                snippet: snippet.into(),
            },
        }
    }

    pub fn at(message: impl Into<String>, span: MdSpan) -> Self {
        MdError {
            message: message.into(),
            span,
        }
    }

    pub fn render(&self) -> String {
        let line = self.span.line.to_string();
        let pad = " ".repeat(line.len());
        let caret = " ".repeat(self.span.col.saturating_sub(1)) + "^";
        format!(
            "error: {}\n  --> markdown:{}:{}\n  {} |\n{} | {}\n  {} {}\n",
            self.message, self.span.line, self.span.col, pad, line, self.span.snippet, pad, caret,
        )
    }
}

impl MdSpan {
    /// Render a rustc-style note for this span, mentioning the failing
    /// document path (e.g. `slides[0].shapes[1]`).
    pub fn render_at(&self, path: &str) -> String {
        let line = self.line.to_string();
        let pad = " ".repeat(line.len());
        let caret = " ".repeat(self.col.saturating_sub(1)) + "^";
        format!(
            "  --> markdown:{}:{}\n  {} |\n{} | {}\n  {} {} (while updating '{path}')\n",
            self.line, self.col, pad, line, self.snippet, pad, caret,
        )
    }
}

impl fmt::Display for MdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.span.line, self.span.col, self.message)
    }
}

impl std::error::Error for MdError {}

pub type MdResult<T> = Result<T, MdError>;
