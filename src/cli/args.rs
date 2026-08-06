use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "pptx-engineer",
    about = "Lossless JSON editing of PowerPoint (.pptx) files",
    after_help = concat!(
        "\u{1b}[1;4mWorkflow:\u{1b}[0m\n",
        r#"  Everything is driven by the pretty-printed JSON snapshot that `jsonfy`
  emits. You edit the JSON and apply it back with `update`.

    1. pptx-engineer jsonfy --input deck.pptx > deck.json
    2. Edit deck.json (change values, remove keys to delete fields, set a
       field to null to delete it, remove or append array elements).
    3. pptx-engineer update --input deck.pptx --json deck.json --output out.pptx

  `update` diffs your JSON against the original file and applies only the
  changes: fields you did not touch are left exactly as they are, and
  unmodeled XML (shadows, gradients, effects) survives untouched.

"#,
        "\u{1b}[1;4mEditing:\u{1b}[0m\n",
        r#"  The JSON snapshot is a full copy of the deck. Edit any value and
  `update` applies it; remove a key (or set it to null) to delete that
  field from the output.

    change a value        edit the JSON and save
    delete a field        remove its key (or set it to null)
    delete an array item  remove the element
    add an array item     append a new element

  This works for shapes, text frames, tables, charts, fills, outlines,
  notes slides, and document properties. Arrays are positional: element 0
  of the JSON matches element 0 of the deck. Read-only fields (shape_id,
  shape_type, image, slide_layout, ...) and fields the schema requires
  (e.g. theme colors) error if you change or remove them.

"#,
        "\u{1b}[1;4mExamples:\u{1b}[0m\n",
        r#"  pptx-engineer jsonfy --input deck.pptx > deck.json
  pptx-engineer update --input deck.pptx --json deck.json --output out.pptx"#
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
    /// Dump a presentation to a pretty-printed JSON snapshot
    #[command(after_help = concat!(
        "\u{1b}[1;4mExamples:\u{1b}[0m\n",
        r#"  pptx-engineer jsonfy --input deck.pptx > deck.json
  pptx-engineer jsonfy --input deck.pptx --media media/ > deck.json

The snapshot is the presentation's editable JSON view: slides and their
shapes (text frames, tables, charts), masters and layouts, theme, core
properties and slide size. Pass --media <DIR> to also write every image
referenced by the deck into DIR."#
    ))]
    Jsonfy {
        /// Input PPTX file path
        #[arg(long)]
        input: String,

        /// Directory to extract media files into
        #[arg(long)]
        media: Option<String>,
    },
    /// Apply a JSON snapshot (as produced by jsonfy) to the presentation
    #[command(after_help = concat!(
        "\u{1b}[1;4mExamples:\u{1b}[0m\n",
        r#"  pptx-engineer update --input deck.pptx --json deck.json --output out.pptx

Only the fields you changed in the JSON are applied; the rest of the deck
is left byte-for-byte intact. The original deck is never modified."#
    ))]
    Update {
        /// Input PPTX file path (the original, used for lossless diffing)
        #[arg(long)]
        input: String,

        /// JSON snapshot to apply (in the shape of jsonfy output)
        #[arg(long)]
        json: String,

        /// Output PPTX file path
        #[arg(long)]
        output: String,
    },
}
