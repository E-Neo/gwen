mod cli;
mod commands;
mod diag;

use clap::Parser;
use cli::Commands;

fn main() -> miette::Result<()> {
    miette::set_hook(Box::new(|_| {
        Box::new(miette::MietteHandlerOpts::new().build())
    }))?;

    let cli = cli::Cli::parse();

    match cli.command {
        Commands::New { project, pptx } => commands::new::execute(&project, &pptx)?,
        Commands::Build { project } => commands::build::execute(Some(&project))?,
    }

    Ok(())
}
