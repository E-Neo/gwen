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
        Commands::Markdown { input, media } => {
            commands::markdown::execute(&input, media.as_deref())?;
        }
        Commands::Build {
            input,
            markdown,
            output,
        } => {
            commands::build::execute(&input, &markdown, &output)?;
        }
    }

    Ok(())
}
