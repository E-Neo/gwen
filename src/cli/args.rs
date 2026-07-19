use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "pptx-engineer",
    about = "Lossless query and modification of PowerPoint (.pptx) files"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Export JSON for a specific path (extracts media to disk)
    Query {
        /// Input PPTX file path
        input: String,

        /// Dot-notation path
        #[arg(long)]
        path: String,

        /// Directory to extract media files into
        #[arg(long)]
        media: Option<String>,
    },
    /// Insert a new element
    Add {
        /// Input PPTX file path
        input: String,

        /// Dot-notation path
        #[arg(long)]
        path: String,

        /// JSON value describing the new element
        #[arg(long)]
        value: String,

        /// Output PPTX file path
        #[arg(long)]
        output: String,
    },
    /// Delete the target element
    Remove {
        /// Input PPTX file path
        input: String,

        /// Dot-notation path
        #[arg(long)]
        path: String,

        /// Output PPTX file path
        #[arg(long)]
        output: String,
    },
    /// Replace the target content
    Replace {
        /// Input PPTX file path
        input: String,

        /// Dot-notation path
        #[arg(long)]
        path: String,

        /// New value
        #[arg(long)]
        value: String,

        /// Output PPTX file path
        #[arg(long)]
        output: String,
    },
    /// Move a subtree (copy + delete within the same package)
    Move {
        /// Input PPTX file path
        input: String,

        /// Source path
        #[arg(long)]
        from: String,

        /// Destination path
        #[arg(long)]
        to: String,

        /// Output PPTX file path
        #[arg(long)]
        output: String,
    },
    /// Copy a subtree
    Copy {
        /// Input PPTX file path
        input: String,

        /// Source path
        #[arg(long)]
        from: String,

        /// Destination path
        #[arg(long)]
        to: String,

        /// Output PPTX file path
        #[arg(long)]
        output: String,
    },
}
