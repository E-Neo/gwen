mod cli;
mod commands;

use clap::Parser;
use cli::Commands;
use gwen_pptx::error::AppResult;

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
