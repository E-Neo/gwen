//! Error handling: everything is a `miette` diagnostic so the CLI can print
//! rich, located messages.

pub type Result<T> = miette::Result<T>;
