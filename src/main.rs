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
title = "deck"
author = ""
company = ""
subject = ""

[layout]
name = "GWEN"
width = "13.333in"
height = "7.5in"

[theme]
major_font = "Arial Black"
minor_font = "Arial"

# Slide index: each section lists its slide files in order.
# [[sections]]
# title = "Intro"
# slides = ["slides/intro.toml"]

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

const DEFAULT_MASTER: &str = r##"# masters/title_base.toml — the master name is the file stem.

background = { color = "FFFFFF" }
margin = 0.5

[[objects]]
type = "rect"
x = "0in"
y = "0in"
w = "13.333in"
h = "1.167in"
fill = { color = "C7000A" }
line = { color = "C7000A", width = 0 }

[[objects]]
type = "text"
x = "0.8in"
y = "0.25in"
w = "11.7in"
h = "0.667in"
text = "Acme Inc."
font_size = 14
color = "FFFFFF"
bold = true
"##;

const DEFAULT_TITLE: &str = r##"# slides/title.toml

master = "title_base"

[[shapes]]
type = "text"
x = "1in"
y = "2.6in"
w = "11.3in"
h = "1in"
text = "*Welcome to* **gwen**"
align = "center"
valign = "middle"
font_size = 44
"##;

const DEFAULT_INTRO: &str = r##"# slides/intro.toml

master = "title_base"

[[shapes]]
type = "text"
x = "0.8in"
y = "1.5in"
w = "11.7in"
h = "5in"
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
    write_file(&root.join("main.toml"), DEFAULT_MAIN)?;
    write_file(
        &root.join("masters").join("title_base.toml"),
        DEFAULT_MASTER,
    )?;
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
