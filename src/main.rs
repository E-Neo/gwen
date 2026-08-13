mod cli;
mod commands;
mod dto;
mod engine;
mod error;
mod md;
mod model;
mod opc;
mod path;

use clap::Parser;
use cli::Commands;
use error::AppResult;

fn main() -> AppResult<()> {
    let cli = cli::Cli::parse();

    match cli.command {
        Commands::Markdown { input, media } => {
            commands::markdown::execute(&input, media.as_deref())?;
        }
        Commands::Update {
            input,
            markdown,
            output,
        } => {
            commands::update::execute(&input, &markdown, &output)?;
        }
    }

    Ok(())
}
