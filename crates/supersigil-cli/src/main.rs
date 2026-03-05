use std::process::ExitCode;

use clap::Parser;
use supersigil_cli::Cli;

fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = run(&cli);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

fn run(cli: &Cli) -> Result<(), supersigil_cli::error::CliError> {
    // Import doesn't need a project config; all other commands do.
    if let supersigil_cli::Command::Import(ref args) = cli.command {
        return supersigil_cli::commands::import::run(args);
    }

    let config_path = supersigil_cli::find_config(&std::env::current_dir()?)?;

    match cli.command {
        supersigil_cli::Command::Lint => {
            let clean = supersigil_cli::commands::lint::run(&config_path)?;
            if !clean {
                return Err(supersigil_cli::error::CliError::LintFailed);
            }
        }
        supersigil_cli::Command::Ls(ref args) => {
            supersigil_cli::commands::ls::run(args, &config_path)?;
        }
        supersigil_cli::Command::Plan(ref args) => {
            supersigil_cli::commands::plan::run(args, &config_path)?;
        }
        supersigil_cli::Command::Context(ref args) => {
            supersigil_cli::commands::context::run(args, &config_path)?;
        }
        supersigil_cli::Command::Import(_) => unreachable!(),
    }

    Ok(())
}
