//! Supersigil CLI binary entry point.

use std::process::ExitCode;

use clap::Parser;
use supersigil_cli::error::CliError;
use supersigil_cli::{Cli, Command};

fn main() -> ExitCode {
    let cli = Cli::parse();

    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            if is_broken_pipe(&e) {
                return ExitCode::SUCCESS;
            }
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

fn is_broken_pipe(err: &CliError) -> bool {
    matches!(err, CliError::Io(io_err) if io_err.kind() == std::io::ErrorKind::BrokenPipe)
}

fn run(cli: &Cli) -> Result<(), CliError> {
    match cli.command {
        Command::Completions(ref args) => supersigil_cli::commands::completions::run(args),
    }
}
