use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("ZIP error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("XML error: {0}")]
    Xml(#[from] quick_xml::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Path parse error: {0}")]
    PathParse(String),

    #[error("Shape index {0} out of bounds")]
    ShapeIndexOutOfBounds(usize),

    #[error("Slide index {0} out of bounds")]
    SlideIndexOutOfBounds(usize),

    #[error("Part not found: {0}")]
    PartNotFound(String),

    #[error("Invalid value: {0}")]
    InvalidValue(String),

    #[error("{0}")]
    Markdown(String),
}

pub type AppResult<T> = Result<T, AppError>;
