//! The `gwen` command line: `new` scaffolds a TOML project (from a user
//! template when one exists), `build` compiles it.

mod scaffold;

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
    /// Create a new gwen project (from GWEN_HOME/template when it exists).
    New {
        /// Project directory to create.
        project: String,
        /// Ignore the user template and use the built-in scaffold.
        #[arg(long)]
        no_template: bool,
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
        Commands::New {
            project,
            no_template,
        } => new_project(&project, no_template),
        Commands::Build { project } => {
            let out = gwen::build(Path::new(&project))?;
            eprintln!("built {}", out.display());
            Ok(())
        }
    }
}

fn new_project(project: &str, no_template: bool) -> Result<()> {
    let root = Path::new(project);
    if root.exists() {
        return Err(miette::miette!("`{project}` already exists"));
    }
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| project.to_string());
    scaffold::scaffold(root, &name, no_template)
}
