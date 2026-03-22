use std::collections::BTreeMap;
use std::path::Path;

use supersigil_core::{SpanKind, xml_escape};
use supersigil_verify::{ExampleOutcome, ExampleResult, MatchCheck, MatchFormat};

struct Patch<'a> {
    start: usize,
    end: usize,
    replacement: &'a str,
    kind: SpanKind,
}

/// Update snapshot files by replacing `body_span` content with the actual output
/// from failed example results.
///
/// Only rewrites `Expected` blocks that use `format="snapshot"`, matching the
/// spec requirement (req-3-4). Source files are normalized (BOM strip, CRLF->LF)
/// before applying byte offsets, since offsets are computed against the
/// normalized source by the parser.
pub(crate) fn update_snapshots(results: &[ExampleResult], emit_warnings: bool) {
    let mut patches: BTreeMap<&Path, Vec<Patch<'_>>> = BTreeMap::new();

    for result in results {
        let Some(ref expected) = result.spec.expected else {
            continue;
        };
        if expected.format != MatchFormat::Snapshot {
            continue;
        }
        let Some(span) = expected.body_span else {
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
            .push(Patch {
                start: span.start,
                end: span.end,
                replacement: actual,
                kind: span.kind,
            });
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
        file_patches.sort_by(|a, b| b.start.cmp(&a.start));

        let mut skipped = false;
        for patch in &file_patches {
            if patch.start > patch.end
                || patch.end > source.len()
                || !source.is_char_boundary(patch.start)
                || !source.is_char_boundary(patch.end)
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
            match patch.kind {
                SpanKind::XmlInline => {
                    let escaped = xml_escape(patch.replacement);
                    source.replace_range(patch.start..patch.end, &escaped);
                }
                SpanKind::RefFence => {
                    source.replace_range(patch.start..patch.end, patch.replacement);
                }
            }
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use supersigil_core::{SourcePosition, SpanKind};
    use supersigil_verify::{
        BodySpan, ExampleOutcome, ExampleResult, ExampleSpec, ExpectedSpec, MatchCheck,
        MatchFailure, MatchFormat,
    };

    use super::update_snapshots;

    fn make_snapshot_result(
        source_path: PathBuf,
        body_span: Option<BodySpan>,
        actual: &str,
    ) -> ExampleResult {
        ExampleResult {
            spec: ExampleSpec {
                doc_id: "test/doc".into(),
                example_id: "snap-test".into(),
                lang: "sh".into(),
                runner: "shell".into(),
                verifies: vec![],
                code: "echo test".into(),
                expected: Some(ExpectedSpec {
                    status: None,
                    format: MatchFormat::Snapshot,
                    contains: None,
                    body: Some("old".into()),
                    body_span,
                }),
                timeout: 30,
                env: vec![],
                setup: None,
                position: SourcePosition {
                    byte_offset: 0,
                    line: 1,
                    column: 1,
                },
                source_path,
            },
            outcome: ExampleOutcome::Fail(vec![MatchFailure {
                check: MatchCheck::Body,
                expected: "old".into(),
                actual: actual.into(),
            }]),
            duration: Duration::from_millis(10),
        }
    }

    #[test]
    fn snapshot_rewrite_escapes_xml_inline_content() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("spec.md");
        let content = "prefix____old___suffix";
        std::fs::write(&file, content).unwrap();

        let result = make_snapshot_result(
            file.clone(),
            Some(BodySpan {
                start: 10,
                end: 13,
                kind: SpanKind::XmlInline,
            }),
            "<b>&x</b>",
        );

        update_snapshots(&[result], false);

        let updated = std::fs::read_to_string(&file).unwrap();
        assert_eq!(
            updated, "prefix____&lt;b&gt;&amp;x&lt;/b&gt;___suffix",
            "XML inline body span should have entities escaped"
        );
    }

    #[test]
    fn snapshot_rewrite_does_not_escape_ref_fence_content() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("spec.md");
        let content = "prefix____old___suffix";
        std::fs::write(&file, content).unwrap();

        let result = make_snapshot_result(
            file.clone(),
            Some(BodySpan {
                start: 10,
                end: 13,
                kind: SpanKind::RefFence,
            }),
            "<b>&x</b>",
        );

        update_snapshots(&[result], false);

        let updated = std::fs::read_to_string(&file).unwrap();
        assert_eq!(
            updated, "prefix____<b>&x</b>___suffix",
            "Ref fence body span should NOT be escaped"
        );
    }
}
