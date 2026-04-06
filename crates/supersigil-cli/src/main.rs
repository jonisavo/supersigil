use std::process::ExitCode;

use clap::Parser;
use supersigil_cli::{Cli, ColorConfig, ExitStatus};

fn main() -> ExitCode {
    let cli = Cli::parse();
    let color = ColorConfig::resolve(cli.color);

    let result = run(&cli, color);
    match result {
        Ok(ExitStatus::Success) => ExitCode::SUCCESS,
        Ok(ExitStatus::VerifyFailed) => ExitCode::from(1),
        Ok(ExitStatus::VerifyWarnings) => ExitCode::from(2),
        Err(e) => {
            if is_broken_pipe(&e) {
                return ExitCode::SUCCESS;
            }
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

fn is_broken_pipe(err: &supersigil_cli::error::CliError) -> bool {
    if let supersigil_cli::error::CliError::Io(io_err) = err {
        return io_err.kind() == std::io::ErrorKind::BrokenPipe;
    }
    false
}

fn run(cli: &Cli, color: ColorConfig) -> Result<ExitStatus, supersigil_cli::error::CliError> {
    // Commands that don't need a project config.
    match cli.command {
        supersigil_cli::Command::Import(ref args) => {
            supersigil_cli::commands::import::run(args, color)?;
            return Ok(ExitStatus::Success);
        }
        supersigil_cli::Command::Init(ref args) => {
            supersigil_cli::commands::init::run(args, color)?;
            return Ok(ExitStatus::Success);
        }
        supersigil_cli::Command::Skills(ref args) => {
            supersigil_cli::commands::skills::run(args, color)?;
            return Ok(ExitStatus::Success);
        }
        _ => {}
    }

    let config_path = supersigil_cli::find_config(&std::env::current_dir()?)?;

    match cli.command {
        supersigil_cli::Command::Ls(ref args) => {
            supersigil_cli::commands::ls::run(args, &config_path, color)?;
        }
        supersigil_cli::Command::Schema(ref args) => {
            supersigil_cli::commands::schema::run(args, &config_path, color)?;
        }
        supersigil_cli::Command::Plan(ref args) => {
            supersigil_cli::commands::plan::run(args, &config_path, color)?;
        }
        supersigil_cli::Command::Context(ref args) => {
            supersigil_cli::commands::context::run(args, &config_path, color)?;
        }
        supersigil_cli::Command::Verify(ref args) => {
            return supersigil_cli::commands::verify::run(args, &config_path, color);
        }
        supersigil_cli::Command::Status(ref args) => {
            supersigil_cli::commands::status::run(args, &config_path, color)?;
        }
        supersigil_cli::Command::Affected(ref args) => {
            supersigil_cli::commands::affected::run(args, &config_path, color)?;
        }
        supersigil_cli::Command::Graph(ref args) => {
            supersigil_cli::commands::graph::run(args, &config_path, color)?;
        }
        supersigil_cli::Command::New(ref args) => {
            supersigil_cli::commands::new::run(args, &config_path, color)?;
        }
        supersigil_cli::Command::Refs(ref args) => {
            supersigil_cli::commands::refs::run(args, &config_path, color)?;
        }
        supersigil_cli::Command::Explore(ref args) => {
            supersigil_cli::commands::explore::run(args, &config_path, color)?;
        }
        supersigil_cli::Command::Render(ref args) => {
            supersigil_cli::commands::render::run(args, &config_path, color)?;
        }
        supersigil_cli::Command::Import(_)
        | supersigil_cli::Command::Init(_)
        | supersigil_cli::Command::Skills(_) => unreachable!(),
    }

    Ok(ExitStatus::Success)
}
