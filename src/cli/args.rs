use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "gwen",
    about = "Lossless Markdown editing of PowerPoint (.pptx) files",
    after_help = concat!(
        "\u{1b}[1;4mWorkflow:\u{1b}[0m\n",
        r#"  Everything is driven by a project directory holding a Markdown mirror
  plus the unmodeled parts captured from the template deck.

    1. gwen new deck --pptx template.pptx
    2. Edit deck/PRESENTATION.md (change text, recolor a run, move or add a
       shape, edit a theme color, add or delete a slide, ...).
    3. gwen build deck   # -> deck/target/<name>.pptx

  `build` regenerates the whole deck from the mirror and the captured parts:
  things you did not model in Markdown (shadows, gradients, effects,
  unmodeled XML) survive untouched because they were captured verbatim.
"#,
        "\u{1b}[1;4mMarkdown editing:\u{1b}[0m\n",
        r#"  The mirror is plain Markdown plus HTML comment markers. The full grammar
  is documented in docs/mirror-format.md and in a legend at the top of every
  mirror.

  PRESENTATION.md lists slides, masters and layouts as `## ` sections
  pointing at per-slide files in src/slides/, src/masters/, src/layouts/.
  A YAML front matter holds the presentation name, slide geometry, theme and
  core properties.

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
        r#"  gwen new deck --pptx template.pptx
  gwen build deck"#
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
    /// Create a new project directory from a template deck
    #[command(after_help = concat!(
        "\u{1b}[1;4mExamples:\u{1b}[0m\n",
        r#"  gwen new deck --pptx template.pptx

Creates deck/ with a Markdown mirror (PRESENTATION.md plus src/slides,
src/masters, src/layouts), the template's unmodeled parts captured into
src/parts, extracted images into src/media, and a config.toml.
Errors if deck/ already exists."#
    ))]
    New {
        /// New project directory (must not exist yet)
        project: String,

        /// Template PPTX file to initialize the project from
        #[arg(long)]
        pptx: String,
    },
    /// Compile a project directory into target/<name>.pptx
    #[command(after_help = concat!(
        "\u{1b}[1;4mExamples:\u{1b}[0m\n",
        r#"  gwen build deck

Rebuilds deck/target/<name>.pptx from the Markdown mirror and the captured
parts. The project name comes from [presentation] name in config.toml."#
    ))]
    Build {
        /// Project directory (defaults to the current directory)
        #[arg(default_value = ".")]
        project: String,
    },
}
