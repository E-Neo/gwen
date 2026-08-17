pub mod error;
pub mod markers;
pub mod normalize;
pub mod parse;
pub mod serialize;
pub mod style;

pub use error::{MdError, MdResult, MdSpan};
pub use parse::{ParsedDoc, parse};
pub use serialize::serialize;
