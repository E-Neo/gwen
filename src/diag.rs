use std::fmt;

use miette::{Diagnostic, NamedSource, SourceCode};

/// A miette diagnostic. With `src`/`span` set it points into the markdown
/// mirror (rendered as an annotated code region); otherwise it is a plain
/// message.
#[derive(Debug)]
pub struct Diag {
    message: String,
    src: Option<NamedSource<String>>,
    help: Option<String>,
}

impl Diag {
    pub fn plain(message: impl Into<String>) -> Self {
        Diag {
            message: message.into(),
            src: None,
            help: None,
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

    fn labels(&self) -> Option<Box<dyn Iterator<Item = miette::LabeledSpan> + '_>> {
        None
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
