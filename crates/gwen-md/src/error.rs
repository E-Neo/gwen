use std::fmt;

/// A source location in the markdown document. Byte `offset`/`len` anchor the
/// location for miette-style diagnostics; `line`/`col` are the
/// human-readable view.
#[derive(Debug, Clone)]
pub struct MdSpan {
    /// 1-based line in the original markdown.
    pub line: usize,
    /// 1-based column.
    pub col: usize,
    /// Byte offset of the location in the original source.
    pub offset: usize,
    /// Length of the located region in bytes.
    pub len: usize,
}

/// A diagnostic attached to a location in the markdown document.
#[derive(Debug, Clone)]
pub struct MdError {
    pub message: String,
    pub span: MdSpan,
}

impl MdError {
    pub fn at(message: impl Into<String>, span: MdSpan) -> Self {
        MdError {
            message: message.into(),
            span,
        }
    }

    pub fn io(err: std::io::Error) -> Self {
        MdError::at(
            format!("I/O error: {err}"),
            MdSpan {
                line: 0,
                col: 0,
                offset: 0,
                len: 0,
            },
        )
    }
}

impl From<std::io::Error> for MdError {
    fn from(err: std::io::Error) -> Self {
        MdError::io(err)
    }
}

impl fmt::Display for MdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.span.line, self.span.col, self.message)
    }
}

impl std::error::Error for MdError {}

pub type MdResult<T> = Result<T, MdError>;
