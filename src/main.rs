//! The `gwen` command line: `new` scaffolds a project, `build` compiles it.

use std::path::Path;

use clap::{Parser, Subcommand};

use gwen::error::Result;

#[derive(Parser, Debug)]
#[command(
    name = "gwen",
    version,
    about = "Generate clean PowerPoint decks from Markdown",
    subcommand_required = true,
    arg_required_else_help = true
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Create a new gwen project.
    New {
        /// Project directory to create.
        project: String,
    },
    /// Build `target/<name>.pptx` from a project.
    Build {
        /// Project directory (defaults to the current directory).
        #[arg(default_value = ".")]
        project: String,
    },
}

fn main() -> miette::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::New { project } => new_project(&project),
        Commands::Build { project } => {
            let out = gwen::build(Path::new(&project))?;
            eprintln!("built {}", out.display());
            Ok(())
        }
    }
}

const DEFAULT_CONFIG: &str = r##"[presentation]
name = "deck"
slide_width = 12192000
slide_height = 6858000
default_layout = "title"

[theme]
major_font = "Calibri"
minor_font = "Calibri"

[[layouts.title.elements]]
kind = "slot"
slot = "title"
type = "text"
left = 914400
top = 2743200
width = 10363200
height = 1371600
align = "center"
anchor = "middle"
text_size = 40
color = "#1D1D1A"

[[layouts.title.elements]]
kind = "slot"
slot = "subtitle"
type = "text"
left = 914400
top = 4200000
width = 10363200
height = 685800
align = "center"
anchor = "top"
text_size = 20
color = "#595959"

[[layouts.content.elements]]
kind = "slot"
slot = "title"
type = "text"
left = 914400
top = 685800
width = 10363200
height = 914400
align = "left"
anchor = "middle"
text_size = 32
color = "#1D1D1A"
bold = true

[[layouts.content.elements]]
kind = "slot"
slot = "body"
type = "text"
left = 914400
top = 1828800
width = 10363200
height = 4114800
align = "left"
anchor = "top"
text_size = 20
color = "#262626"
"##;

const DEFAULT_SUMMARY: &str =
    "# Summary\n\n- [Title](slides/title.md)\n- [Content](slides/content.md)\n";

const DEFAULT_TITLE: &str = "---\nlayout: title\n---\n\n# My Deck\n\n## A gwen presentation\n";

const DEFAULT_CONTENT: &str =
    "---\nlayout: content\n---\n\n# First slide\n\n- point one\n- point two\n";

fn new_project(project: &str) -> Result<()> {
    let root = Path::new(project);
    if root.exists() {
        return Err(miette::miette!("`{project}` already exists"));
    }
    let src = root.join("src");
    create_dir(&src.join("slides"))?;
    create_dir(&src.join("media"))?;
    write_file(&root.join("config.toml"), DEFAULT_CONFIG)?;
    write_file(&src.join("SUMMARY.md"), DEFAULT_SUMMARY)?;
    write_file(&src.join("slides").join("title.md"), DEFAULT_TITLE)?;
    write_file(&src.join("slides").join("content.md"), DEFAULT_CONTENT)?;
    eprintln!("created project `{project}`");
    eprintln!(
        "  edit {project}/src/SUMMARY.md and src/slides/*.md, then run `gwen build {project}`"
    );
    Ok(())
}

fn create_dir(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)
        .map_err(|e| miette::miette!("cannot create {}: {e}", path.display()))
}

fn write_file(path: &Path, contents: &str) -> Result<()> {
    std::fs::write(path, contents)
        .map_err(|e| miette::miette!("cannot write {}: {e}", path.display()))
}
