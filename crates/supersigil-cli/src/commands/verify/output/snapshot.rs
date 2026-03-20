use std::collections::BTreeMap;
use std::path::Path;

use supersigil_verify::{ExampleOutcome, ExampleResult, MatchCheck, MatchFormat};

/// Update snapshot files by replacing `body_span` content with the actual output
/// from failed example results.
///
/// Only rewrites `Expected` blocks that use `format="snapshot"`, matching the
/// spec requirement (req-3-4). Source files are normalized (BOM strip, CRLF->LF)
/// before applying byte offsets, since offsets are computed against the
/// normalized source by the parser.
pub(crate) fn update_snapshots(results: &[ExampleResult], emit_warnings: bool) {
    let mut patches: BTreeMap<&Path, Vec<(usize, usize, &str)>> = BTreeMap::new();

    for result in results {
        let Some(ref expected) = result.spec.expected else {
            continue;
        };
        if expected.format != MatchFormat::Snapshot {
            continue;
        }
        let Some((start, end)) = expected.body_span else {
            continue;
        };

        let actual_output = match &result.outcome {
            ExampleOutcome::Fail(failures) => failures
                .iter()
                .find(|failure| failure.check == MatchCheck::Body)
                .map(|failure| failure.actual.as_str()),
            _ => continue,
        };

        let Some(actual) = actual_output else {
            continue;
        };

        patches
            .entry(&result.spec.source_path)
            .or_default()
            .push((start, end, actual));
    }

    for (source_path, mut file_patches) in patches {
        let Ok(raw_source) = std::fs::read_to_string(source_path) else {
            if emit_warnings {
                eprintln!(
                    "warning: could not read {} for snapshot update",
                    source_path.display()
                );
            }
            continue;
        };

        let mut source = supersigil_parser::normalize(&raw_source);
        file_patches.sort_by(|a, b| b.0.cmp(&a.0));

        let mut skipped = false;
        for (start, end, actual) in &file_patches {
            if *start > *end
                || *end > source.len()
                || !source.is_char_boundary(*start)
                || !source.is_char_boundary(*end)
            {
                if emit_warnings {
                    eprintln!(
                        "warning: stale byte offsets for snapshot in {}, skipping update",
                        source_path.display()
                    );
                }
                skipped = true;
                break;
            }
            source.replace_range(*start..*end, actual);
        }

        if skipped {
            continue;
        }

        if let Err(error) = std::fs::write(source_path, &source)
            && emit_warnings
        {
            eprintln!(
                "warning: could not write snapshot update to {}: {error}",
                source_path.display()
            );
        }
    }
}
