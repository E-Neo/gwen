pub mod error;
pub mod markers;
pub mod normalize;
pub mod parse;
pub mod serialize;
pub mod style;

pub use error::{MdError, MdResult, MdSpan};
pub use parse::{FileKind, ParsedDoc, parse, parse_file, read_document};
pub use serialize::{serialize, write_document};
