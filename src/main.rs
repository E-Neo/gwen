//! The `gwen` command line: `new` scaffolds a TOML project (from a user
//! template when one exists), `build` compiles it.

mod fmt;
mod scaffold;
mod update;

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
    /// Sync a project with the installed template: refresh `masters/`,
    /// `media/` and the template's skills, and merge its `[theme]` /
    /// `[styles.*]` into `main.toml`. Slides, sections and the presentation
    /// title are never touched.
    Update {
        /// Project directory (defaults to the current directory).
        #[arg(default_value = ".")]
        project: String,
        /// Report what would change without writing anything.
        #[arg(long)]
        dry_run: bool,
        /// Leave `main.toml` (theme and styles) as it is.
        #[arg(long)]
        keep_styles: bool,
    },
    /// Format the TOML files in a project: canonical spacing, one blank line
    /// before table headers, no trailing whitespace. Values, key order and
    /// comments are never changed; multi-line strings are kept verbatim.
    Fmt {
        /// Project directory (defaults to the current directory).
        #[arg(default_value = ".")]
        project: String,
        /// Report what would change without writing anything; exits 1 when a
        /// file needs formatting.
        #[arg(long)]
        check: bool,
    },
    /// Build `target/<title>.pptx` from a project.
    Build {
        /// Project directory (defaults to the current directory).
        #[arg(default_value = ".")]
        project: String,
    },
    /// Remove the generated `target/` directory.
    Clean {
        /// Project directory (defaults to the current directory).
        #[arg(default_value = ".")]
        project: String,
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
        /// Also write `target/render/index.html`, a self-contained slideshow
        /// embedding the first format. Press `f` there to present full screen.
        #[arg(long)]
        html: bool,
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
        Commands::Update {
            project,
            dry_run,
            keep_styles,
        } => update::update(&project, dry_run, keep_styles),
        Commands::Fmt { project, check } => {
            if fmt::format(&project, check)? {
                std::process::exit(1);
            }
            Ok(())
        }
        Commands::Build { project } => {
            let out = gwen::build(Path::new(&project))?;
            eprintln!("built {}", out.display());
            Ok(())
        }
        Commands::Clean { project } => {
            let root = Path::new(&project);
            if gwen::clean(root)? {
                eprintln!("cleaned {}", root.join("target").display());
            } else {
                eprintln!("nothing to clean");
            }
            Ok(())
        }
        Commands::Render {
            project,
            page,
            format,
            html,
        } => {
            let pages = match page.as_deref() {
                Some(raw) => {
                    Some(gwen::pages::PageSelection::parse(raw).map_err(|e| miette::miette!(e))?)
                }
                None => None,
            };
            let formats = gwen::preview::Formats::parse(&format).map_err(|e| miette::miette!(e))?;
            let out = gwen::preview::render(Path::new(&project), pages.as_ref(), formats, html)?;
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
