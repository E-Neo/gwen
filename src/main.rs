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
    about = "Generate PowerPoint decks from TOML",
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
        /// Import masters/slides/media (and main.toml) from this `.pptx`
        /// instead of the template/built-in scaffold.
        #[arg(long)]
        pptx: Option<String>,
    },
    /// Build `target/<title>.pptx` from a project.
    Build {
        /// Project directory (defaults to the current directory).
        #[arg(default_value = ".")]
        project: String,
    },
    /// Measure composition, content, structure and palette facts for every
    /// slide (or one `--page`), as Markdown by default or JSON with `--json`.
    Measure {
        /// Project directory (defaults to the current directory).
        #[arg(default_value = ".")]
        project: String,
        /// Emit a JSON document instead of Markdown.
        #[arg(long)]
        json: bool,
        /// Measure selected slides only, as 1-based numbers and ranges
        /// (`1`, `1,3-5`).
        #[arg(long, value_name = "N[,N-R]")]
        page: Option<String>,
    },
    /// Render slides to `target/render/<slide-stem>.svg` and/or `.png`
    /// (no office suite required; text is drawn with gwen's own layout).
    Render {
        /// Project directory (defaults to the current directory).
        #[arg(default_value = ".")]
        project: String,
        /// Render selected slides only, as 1-based numbers and ranges
        /// (`1`, `1,3-5`).
        #[arg(long, value_name = "N[,N-R]")]
        page: Option<String>,
        /// Output formats, comma separated (`png`, `svg`). PNG is the
        /// default; SVG is only written when requested.
        #[arg(long, value_name = "LIST", default_value = "png")]
        format: String,
    },
}

fn main() -> miette::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::New {
            project,
            no_template,
            pptx,
        } => new_project(&project, no_template, pptx.as_deref()),
        Commands::Build { project } => {
            let out = gwen::build(Path::new(&project))?;
            eprintln!("built {}", out.display());
            Ok(())
        }
        Commands::Measure {
            project,
            json,
            page,
        } => {
            let pages = match page.as_deref() {
                Some(raw) => {
                    Some(gwen::pages::PageSelection::parse(raw).map_err(|e| miette::miette!(e))?)
                }
                None => None,
            };
            let m = gwen::measure::measure_with(Path::new(&project), pages.as_ref())?;
            print!("{}", if json { m.to_json() } else { m.to_markdown() });
            Ok(())
        }
        Commands::Render {
            project,
            page,
            format,
        } => {
            let pages = match page.as_deref() {
                Some(raw) => {
                    Some(gwen::pages::PageSelection::parse(raw).map_err(|e| miette::miette!(e))?)
                }
                None => None,
            };
            let formats = gwen::preview::Formats::parse(&format).map_err(|e| miette::miette!(e))?;
            let out = gwen::preview::render(Path::new(&project), pages.as_ref(), formats)?;
            for path in out {
                eprintln!("wrote {}", path.display());
            }
            Ok(())
        }
    }
}

fn new_project(project: &str, no_template: bool, pptx: Option<&str>) -> Result<()> {
    let root = Path::new(project);
    if root.exists() {
        return Err(miette::miette!("`{project}` already exists"));
    }
    if let Some(pptx) = pptx {
        let src = Path::new(pptx);
        if !src.is_file() {
            return Err(miette::miette!("`{pptx}` is not a file"));
        }
    }
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| project.to_string());
    scaffold::scaffold(root, &name, no_template, pptx.map(Path::new))
}
