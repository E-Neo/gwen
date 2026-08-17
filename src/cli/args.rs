use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "gwen",
    about = "Lossless Markdown editing of PowerPoint (.pptx) files",
    after_help = concat!(
        "\u{1b}[1;4mWorkflow:\u{1b}[0m\n",
        r#"  Everything is driven by the Markdown mirror that `markdown` emits. You
  edit the Markdown and rebuild the deck with `build`.

    1. gwen markdown --input deck.pptx > deck.md
    2. Edit deck.md (change text, recolor a run, move or add a shape,
       edit a theme color, add or delete a slide, ...).
    3. gwen build --input deck.pptx --markdown deck.md --output out.pptx

  `build` diffs your Markdown against the original file and applies only the
  changes: things you did not touch are left exactly as they are, and
  unmodeled XML (shadows, gradients, effects) survives untouched.

"#,
        "\u{1b}[1;4mMarkdown editing:\u{1b}[0m\n",
        r#"  The mirror is plain Markdown plus HTML comment markers. The full grammar
  is documented in docs/mirror-format.md and in a legend at the top of every
  mirror.

  Slides are separated by `## ` headings (structural: `## Slide 2`; the text
  is only a label). Masters are `# Master N`, notes `### Notes`. A `<style>`
  block at the top (plus a YAML front matter with slide geometry, theme and
  core properties) defines classes that pin the unmodeled XML.

  Each shape carries one readable marker followed by its content (markdown
  paragraphs, or a markdown table for tables):

    <!-- shape type="textbox" name="TextBox 1" left="914400" top="914400"
            width="3657600" height="914400" -->

  The class holds only styling (fill, outline, text-frame and default
  paragraph properties); identity and geometry live in the marker's
  attributes. Geometry is EMU (914400 EMU = 1 inch); inches, points, cm, mm
  and px are accepted when editing. Inline formatting is native Markdown
  emphasis plus `<span>` classes from the style block:

    **bold text**, *italic*, <span class="run-1">big</span>

  Editing rules:
    - remove a shape's block to delete it
    - append a new shape by copying an existing block and editing it
    - add a slide by writing a new `## ` section; remove one by deleting it
    - `<!-- paragraph class="..." -->` overrides one paragraph's style when
      it differs from the shape's default
    - read-only fields (shape_id, slide layouts, table row heights, cell
      paragraph styles) are not emitted and are preserved on build
    - removing a required field (e.g. a theme color) is an error

"#,
        "\u{1b}[1;4mExamples:\u{1b}[0m\n",
        r#"  gwen markdown --input deck.pptx > deck.md
  gwen build --input deck.pptx --markdown deck.md --output out.pptx"#
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
        r#"  gwen markdown --input deck.pptx > deck.md
  gwen markdown --input deck.pptx --media media/ > deck.md

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
        r#"  gwen build --input deck.pptx --markdown deck.md --output out.pptx

Only the parts you changed are applied; the rest of the deck is left
byte-for-byte intact. The original deck is never modified."#
    ))]
    Build {
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
