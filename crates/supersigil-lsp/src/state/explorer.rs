use std::collections::{BTreeSet, HashSet};

#[allow(
    clippy::wildcard_imports,
    reason = "task scaffold keeps explorer helpers aligned with the parent module imports"
)]
use super::*;

impl SupersigilLsp {
    pub(super) fn current_explorer_revision(&self) -> String {
        self.explorer_revision.to_string()
    }

    pub(super) fn build_explorer_snapshot_for_revision(
        &self,
        revision: &str,
    ) -> crate::explorer_runtime::ExplorerSnapshot {
        crate::explorer_runtime::build_explorer_snapshot(
            &crate::explorer_runtime::BuildExplorerSnapshotInput {
                revision,
                graph: &self.graph,
                evidence_by_target: self.evidence_by_target.as_deref(),
                project_root: self.project_root.as_deref().unwrap_or(Path::new("")),
            },
        )
    }

    pub(super) fn current_explorer_snapshot(&self) -> crate::explorer_runtime::ExplorerSnapshot {
        self.last_explorer_snapshot.clone().unwrap_or_else(|| {
            self.build_explorer_snapshot_for_revision(&self.current_explorer_revision())
        })
    }

    pub(super) fn find_document_by_id(&self, document_id: &str) -> Option<(SpecDocument, bool)> {
        if let Some(doc) = self
            .file_parses
            .values()
            .find(|doc| doc.frontmatter.id == document_id)
        {
            return Some((doc.clone(), false));
        }

        self.partial_file_parses
            .values()
            .find(|doc| doc.frontmatter.id == document_id)
            .map(|doc| (doc.clone(), true))
    }

    pub(super) fn read_document_content(&self, doc: &SpecDocument) -> Arc<String> {
        crate::path_to_url(&doc.path)
            .and_then(|uri| self.content_from_buffer_or_disk(&uri))
            .unwrap_or_else(|| Arc::new(std::fs::read_to_string(&doc.path).unwrap_or_default()))
    }

    pub(super) fn build_document_components_for_doc(
        &self,
        doc: &SpecDocument,
        stale: bool,
    ) -> crate::document_components::DocumentComponentsResult {
        use crate::document_components::{BuildComponentsInput, build_document_components};

        let content = self.read_document_content(doc);

        build_document_components(&BuildComponentsInput {
            doc,
            stale,
            content: &content,
            graph: &self.graph,
            evidence_by_target: self.evidence_by_target.as_deref(),
            evidence_records: self.evidence_records.as_deref().map(Vec::as_slice),
            project_root: self.project_root.as_deref().unwrap_or(Path::new("")),
        })
    }

    pub(super) fn build_document_fingerprint_for_doc(
        &self,
        doc: &SpecDocument,
        stale: bool,
        evidence_record_lookup: Option<&HashMap<EvidenceId, &VerificationEvidenceRecord>>,
    ) -> crate::explorer_runtime::ExplorerDocumentFingerprint {
        let content = self.read_document_content(doc);
        crate::explorer_runtime::fingerprint_explorer_document_detail(
            &crate::explorer_runtime::FingerprintExplorerDocumentInput {
                document_id: &doc.frontmatter.id,
                stale,
                content: &content,
                components: &doc.components,
                graph: &self.graph,
                evidence_by_target: self.evidence_by_target.as_deref(),
                evidence_record_lookup,
                project_root: self.project_root.as_deref().unwrap_or(Path::new("")),
            },
        )
    }

    pub(super) fn current_explorer_document_fingerprints(
        &self,
        snapshot: &crate::explorer_runtime::ExplorerSnapshot,
    ) -> HashMap<String, crate::explorer_runtime::ExplorerDocumentFingerprint> {
        let evidence_record_lookup = self.evidence_records.as_deref().map(|records| {
            records
                .iter()
                .map(|record| (record.id, record))
                .collect::<HashMap<_, _>>()
        });
        let mut remaining = snapshot
            .documents
            .iter()
            .map(|document| document.id.clone())
            .collect::<HashSet<_>>();
        let mut fingerprints = HashMap::with_capacity(remaining.len());

        for doc in self.file_parses.values() {
            let document_id = doc.frontmatter.id.clone();
            if remaining.remove(&document_id) {
                fingerprints.insert(
                    document_id,
                    self.build_document_fingerprint_for_doc(
                        doc,
                        false,
                        evidence_record_lookup.as_ref(),
                    ),
                );
                if remaining.is_empty() {
                    return fingerprints;
                }
            }
        }

        for doc in self.partial_file_parses.values() {
            let document_id = doc.frontmatter.id.clone();
            if remaining.remove(&document_id) {
                fingerprints.insert(
                    document_id,
                    self.build_document_fingerprint_for_doc(
                        doc,
                        true,
                        evidence_record_lookup.as_ref(),
                    ),
                );
                if remaining.is_empty() {
                    break;
                }
            }
        }

        fingerprints
    }

    pub(super) fn notify_documents_changed(&mut self) {
        let _ = self
            .client
            .notify::<crate::document_list::DocumentsChanged>(());

        let next_revision = self.explorer_revision + 1;
        let snapshot = self.build_explorer_snapshot_for_revision(&next_revision.to_string());
        let detail_fingerprints = self.current_explorer_document_fingerprints(&snapshot);
        let mut event = crate::explorer_runtime::diff_explorer_snapshots(
            self.last_explorer_snapshot.as_ref(),
            &snapshot,
        );
        let detail_changes = crate::explorer_runtime::diff_explorer_documents(
            self.last_explorer_detail_fingerprints.as_ref(),
            &detail_fingerprints,
        );
        if !detail_changes.is_empty() {
            let removed_ids: HashSet<&str> = event
                .removed_document_ids
                .iter()
                .map(String::as_str)
                .collect();
            let mut changed_ids: BTreeSet<String> =
                event.changed_document_ids.iter().cloned().collect();
            for document_id in detail_changes {
                if !removed_ids.contains(document_id.as_str()) {
                    changed_ids.insert(document_id);
                }
            }
            event.changed_document_ids = changed_ids.into_iter().collect();
        }
        let has_change = self.last_explorer_snapshot.is_none()
            || !event.changed_document_ids.is_empty()
            || !event.removed_document_ids.is_empty();

        if has_change {
            self.explorer_revision = next_revision;
            self.last_explorer_snapshot = Some(snapshot);
            self.last_explorer_detail_fingerprints = Some(detail_fingerprints);
            let _ = self
                .client
                .notify::<crate::explorer_runtime::ExplorerChangedNotification>(event);
        }
    }
}
