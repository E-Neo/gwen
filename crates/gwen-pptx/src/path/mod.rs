pub mod resolver;

pub use resolver::{PathSegment, ResolvedPath, resolve_path};

#[cfg(test)]
pub use resolver::parse_path;
