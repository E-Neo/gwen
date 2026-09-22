//! The `gwen` command line: `new` scaffolds a TOML project, `build` compiles it.

use std::path::Path;

use clap::{Parser, Subcommand};

use gwen::error::Result;

#[derive(Parser, Debug)]
#[command(
    name = "gwen",
    version,
    about = "Generate PowerPoint decks from TOML (drives pptxgenjs)",
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
    /// Build `target/<title>.pptx` from a project.
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

const DEFAULT_MAIN: &str = r##"[presentation]
title = "__NAME__"
author = ""
company = ""
subject = ""
width = 12196763
height = 6858000

[theme]
major_font = "Arial Black"
minor_font = "Arial"

# Slide index: [[sections]] is required and lists the slide files in order.
[[sections]]
title = "Intro"
slides = ["title.toml", "intro.toml"]

# Built-in defaults shared by every slide (shape keys win over these).
# [defaults.text]
# font_face = "Arial"
# font_size = 18
# color = "262626"
# [defaults.shape]
# fill = { color = "C7000A" }
# [defaults.image]
# sizing = { type = "contain" }

# Named styles; a shape referencing `style = "muted"` merges these.
# [styles.muted]
# color = "808080"
# italic = true
"##;

const DEFAULT_MASTER: &str = r##"# masters/base.toml — the master name is the file stem.

background = { color = "FFFFFF" }
slide_number = { x = "12.2in", y = "7.1in", w = "1in", h = "0.3in",
                 font_size = 12, color = "999999", align = "right" }

[[shapes]]
type = "rect"
x = 0
y = 0
w = "100%"
h = "1.1in"
fill = { color = "C7000A" }
line = { color = "C7000A", width = 0 }

[[shapes]]
type = "placeholder"
ph_type = "title"
name = "Title"
x = "0.8in"
y = "0.18in"
w = "11.7in"
h = "0.74in"
font_size = 26
color = "FFFFFF"
bold = true
align = "left"
valign = "middle"
text = "Click to edit title"
"##;

const DEFAULT_TITLE: &str = r##"# slides/title.toml

master = "base"

[[shapes]]
type = "text"
placeholder = "Title"
text = "*Welcome to* **gwen**"
"##;

const DEFAULT_INTRO: &str = r##"# slides/intro.toml

master = "base"

[[shapes]]
type = "text"
x = "1in"
y = "1.8in"
w = "11.3in"
h = "4in"
text = """**gwen** builds .pptx from TOML.

It drives the real pptxgenjs under QuickJS, so everything is
[standard](https://gitbrent.github.io/PptxGenJS/) PowerPoint.
"""
font_size = 24
"##;

fn new_project(project: &str) -> Result<()> {
    let root = Path::new(project);
    if root.exists() {
        return Err(miette::miette!("`{project}` already exists"));
    }
    create_dir(&root.join("slides"))?;
    create_dir(&root.join("masters"))?;
    create_dir(&root.join("media"))?;
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| project.to_string());
    write_file(
        &root.join("main.toml"),
        &DEFAULT_MAIN.replace("__NAME__", &name),
    )?;
    write_file(&root.join("masters").join("base.toml"), DEFAULT_MASTER)?;
    write_file(&root.join("slides").join("title.toml"), DEFAULT_TITLE)?;
    write_file(&root.join("slides").join("intro.toml"), DEFAULT_INTRO)?;
    eprintln!("created project `{project}`");
    eprintln!("  edit {project}/main.toml and slides/*.toml, then run `gwen build {project}`");
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
