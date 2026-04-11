use std::io::Write;

use clap::CommandFactory;

use crate::Cli;
use crate::commands::CompletionsArgs;
use crate::error::CliError;

/// Run the `completions` command: print shell completion script to stdout.
///
/// # Errors
///
/// Returns an error if writing to stdout fails.
pub fn run(args: &CompletionsArgs) -> Result<(), CliError> {
    let mut buf = Vec::new();
    let mut cmd = Cli::command();
    clap_complete::generate(args.shell, &mut cmd, "supersigil", &mut buf);
    std::io::stdout().write_all(&buf)?;
    Ok(())
}
