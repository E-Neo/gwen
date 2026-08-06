mod cli;
mod commands;
mod dto;
mod engine;
mod error;
mod model;
mod opc;
mod path;

use clap::Parser;
use cli::Commands;
use error::AppResult;

fn main() -> AppResult<()> {
    let cli = cli::Cli::parse();

    match cli.command {
        Commands::Jsonfy { input, media } => {
            commands::jsonfy::execute(&input, media.as_deref())?;
        }
        Commands::Update {
            input,
            json,
            output,
        } => {
            commands::update::execute(&input, &json, &output)?;
        }
    }

    Ok(())
}
