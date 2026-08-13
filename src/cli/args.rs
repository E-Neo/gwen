use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "pptx-engineer",
    about = "Lossless Markdown editing of PowerPoint (.pptx) files",
    after_help = concat!(
        "\u{1b}[1;4mWorkflow:\u{1b}[0m\n",
        r#"  Everything is driven by the Markdown mirror that `markdown` emits. You
  edit the Markdown and apply it back with `update`.

    1. pptx-engineer markdown --input deck.pptx > deck.md
    2. Edit deck.md (change text, recolor a run, move or add a shape,
       edit a theme color, ...).
    3. pptx-engineer update --input deck.pptx --markdown deck.md --output out.pptx

  `update` diffs your Markdown against the original file and applies only the
  changes: things you did not touch are left exactly as they are, and
  unmodeled XML (shadows, gradients, effects) survives untouched.

"#,
        "\u{1b}[1;4mMarkdown editing:\u{1b}[0m\n",
        r#"  Every slide, master shape and table carries an HTML comment that pins
  the metadata (`<!-- shape: ... -->`, `<!-- table: grid=... -->`). Inline
  formatting is native Markdown emphasis plus `<span>` attributes:

    **bold text**, *italic*, `**<span data-size=2400>big</span>**`
    change a run's color with <span data-color="RGB:FF0000">

  Editing rules:
    - remove a shape's block to delete it
    - append a new shape by copying an existing block and editing it
    - read-only fields (shape_id, slide layouts, table row heights, cell
      paragraph styles) are not emitted and are preserved on update
    - removing a required field (e.g. a theme color) is an error

"#,
        "\u{1b}[1;4mExamples:\u{1b}[0m\n",
        r#"  pptx-engineer markdown --input deck.pptx > deck.md
  pptx-engineer update --input deck.pptx --markdown deck.md --output out.pptx"#
    ),
    subcommand_required = true,
    arg_required_else_help = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Dump a presentation to an editable Markdown mirror
    #[command(after_help = concat!(
        "\u{1b}[1;4mExamples:\u{1b}[0m\n",
        r#"  pptx-engineer markdown --input deck.pptx > deck.md
  pptx-engineer markdown --input deck.pptx --media media/ > deck.md

The mirror covers slides and their shapes (text frames, tables), masters,
theme and core properties. Pass --media <DIR> to also write every image
referenced by the deck into DIR."#
    ))]
    Markdown {
        /// Input PPTX file path
        #[arg(long)]
        input: String,

        /// Directory to extract media files into
        #[arg(long)]
        media: Option<String>,
    },
    /// Apply an edited Markdown mirror to the presentation
    #[command(after_help = concat!(
        "\u{1b}[1;4mExamples:\u{1b}[0m\n",
        r#"  pptx-engineer update --input deck.pptx --markdown deck.md --output out.pptx

Only the parts you changed are applied; the rest of the deck is left
byte-for-byte intact. The original deck is never modified."#
    ))]
    Update {
        /// Input PPTX file path (the original, used for lossless diffing)
        #[arg(long)]
        input: String,

        /// Markdown mirror to apply (as produced by markdown)
        #[arg(long)]
        markdown: String,

        /// Output PPTX file path
        #[arg(long)]
        output: String,
    },
}
