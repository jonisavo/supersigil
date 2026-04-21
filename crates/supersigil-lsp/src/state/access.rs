#[allow(
    clippy::wildcard_imports,
    reason = "state access helpers share the parent imports"
)]
use super::*;

impl SupersigilLsp {
    pub(super) fn indexed_doc_for_uri(&self, uri: &Url) -> Option<&SpecDocument> {
        let rel = self.uri_to_relative_key(uri)?;
        self.file_parses.get(&rel)
    }

    pub(super) fn current_or_partial_doc_for_uri(
        &self,
        uri: &Url,
    ) -> Option<(&SpecDocument, bool)> {
        let rel = self.uri_to_relative_key(uri)?;
        if self.open_files.contains_key(uri)
            && let Some(doc) = self.partial_file_parses.get(&rel)
        {
            return Some((doc, true));
        }
        if let Some(doc) = self.file_parses.get(&rel) {
            return Some((doc, false));
        }
        self.partial_file_parses.get(&rel).map(|doc| (doc, true))
    }

    pub(super) fn indexed_doc_id_for_uri(&self, uri: &Url) -> Option<String> {
        self.indexed_doc_for_uri(uri)
            .map(|doc| doc.frontmatter.id.clone())
    }

    pub(super) fn content_from_open_buffer(&self, uri: &Url) -> Option<Arc<String>> {
        self.open_files.get(uri).cloned()
    }

    pub(super) fn content_from_buffer_or_disk(&self, uri: &Url) -> Option<Arc<String>> {
        if let Some(content) = self.content_from_open_buffer(uri) {
            return Some(content);
        }

        let rel = self.uri_to_relative_key(uri)?;
        let abs = self
            .project_root
            .as_ref()
            .map(|root| root.join(&rel))
            .unwrap_or(rel);
        std::fs::read_to_string(abs).ok().map(Arc::new)
    }
}
