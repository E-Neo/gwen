mod cli;
mod commands;
mod dto;
mod engine;
mod error;
mod model;
mod opc;
mod path;
mod template;

use clap::Parser;
use cli::Commands;
use error::AppResult;

fn main() -> AppResult<()> {
    let cli = cli::Cli::parse();

    match cli.command {
        Commands::Query {
            input,
            path,
            media,
            pretty,
        } => {
            commands::query::execute(&input, &path, media.as_deref(), pretty)?;
        }
        Commands::Add {
            input,
            path,
            value,
            output,
        } => {
            commands::add::execute(&input, &path, &value, &output)?;
        }
        Commands::Remove {
            input,
            path,
            output,
        } => {
            commands::remove::execute(&input, &path, &output)?;
        }
        Commands::Replace {
            input,
            path,
            value,
            output,
        } => {
            commands::replace::execute(&input, &path, &value, &output)?;
        }
        Commands::Move {
            input,
            from,
            to,
            output,
        } => {
            commands::copy_move::move_shape(&input, &from, &to, &output)?;
        }
        Commands::Copy {
            input,
            from,
            to,
            output,
        } => {
            commands::copy_move::copy_shape(&input, &from, &to, &output)?;
        }
        Commands::New { output, size } => {
            let pkg = template::build_default_package(template::SlideSize::parse(&size)?)?;
            pkg.save(std::path::Path::new(&output))?;
        }
    }

    Ok(())
}
