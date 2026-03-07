use std::io::{self, Write};
use std::path::Path;

use supersigil_verify::{
    ResultStatus, VerifyOptions, format_json, format_markdown, format_terminal,
};

use crate::commands::{VerifyArgs, VerifyFormat};
use crate::error::CliError;
use crate::format::{self, ColorConfig, ExitStatus};
use crate::loader;

/// Run the `verify` command: cross-document verification.
///
/// # Errors
///
/// Returns `CliError` if loading fails or verification encounters a fatal error.
pub fn run(
    args: &VerifyArgs,
    config_path: &Path,
    color: ColorConfig,
) -> Result<ExitStatus, CliError> {
    let (config, graph) = loader::load_graph(config_path)?;
    let project_root = loader::project_root(config_path);

    let options = VerifyOptions {
        project: args.project.clone(),
        since_ref: args.since.clone(),
        committed_only: args.committed_only,
        use_merge_base: args.merge_base,
    };

    let report = supersigil_verify::verify(&graph, &config, project_root, &options)?;
    let status = report.result_status();

    let stdout = io::stdout();
    let mut out = stdout.lock();

    match args.format {
        VerifyFormat::Terminal => {
            let text = format_terminal(&report, color.use_color());
            write!(out, "{text}")?;
        }
        VerifyFormat::Json => {
            let text = format_json(&report);
            writeln!(out, "{text}")?;
        }
        VerifyFormat::Markdown => {
            let text = format_markdown(&report);
            write!(out, "{text}")?;
        }
    }

    match status {
        ResultStatus::Clean => {
            let n = report.summary.total_documents;
            eprintln!("{} {n} documents verified, no findings.", color.ok());
            Ok(ExitStatus::Success)
        }
        ResultStatus::HasErrors => {
            format::hint(color, "Run `supersigil plan` to see outstanding work.");
            Ok(ExitStatus::VerifyFailed)
        }
        ResultStatus::WarningsOnly => Ok(ExitStatus::VerifyWarnings),
    }
}
