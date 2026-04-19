use std::collections::HashMap;
use std::path::{Path, PathBuf};

type GlobLoader = fn(&str, &Path) -> Vec<PathBuf>;

#[derive(Debug)]
pub(crate) struct GlobResolver {
    project_root: PathBuf,
    cache: HashMap<String, Vec<PathBuf>>,
    loader: GlobLoader,
}

impl GlobResolver {
    pub(crate) fn new(project_root: &Path) -> Self {
        Self {
            project_root: project_root.to_path_buf(),
            cache: HashMap::new(),
            loader: supersigil_core::expand_glob,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_loader_for_tests(project_root: &Path, loader: GlobLoader) -> Self {
        Self {
            project_root: project_root.to_path_buf(),
            cache: HashMap::new(),
            loader,
        }
    }

    pub(crate) fn expand(&mut self, pattern: &str) -> &[PathBuf] {
        if !self.cache.contains_key(pattern) {
            let matches = (self.loader)(pattern, &self.project_root);
            self.cache.insert(pattern.to_owned(), matches);
        }
        self.cache
            .get(pattern)
            .expect("glob cache entry exists after cache fill")
            .as_slice()
    }

    pub(crate) fn expand_all<'a>(
        &mut self,
        patterns: impl IntoIterator<Item = &'a str>,
    ) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        for pattern in patterns {
            paths.extend(self.expand(pattern).iter().cloned());
        }
        paths
    }
}
