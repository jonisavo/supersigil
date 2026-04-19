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
            let matches = if looks_like_glob(pattern) {
                (self.loader)(pattern, &self.project_root)
            } else {
                expand_literal_path(pattern, &self.project_root)
            };
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

fn looks_like_glob(pattern: &str) -> bool {
    pattern.contains('*') || pattern.contains('?') || pattern.contains('[')
}

fn expand_literal_path(pattern: &str, project_root: &Path) -> Vec<PathBuf> {
    let candidate = project_root.join(pattern);
    if candidate.exists() {
        vec![candidate]
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn literal_existing_path_bypasses_loader() {
        static CALLS: AtomicUsize = AtomicUsize::new(0);

        fn counting_loader(pattern: &str, _base_dir: &Path) -> Vec<PathBuf> {
            CALLS.fetch_add(1, Ordering::SeqCst);
            vec![PathBuf::from(pattern)]
        }

        CALLS.store(0, Ordering::SeqCst);
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("tests/auth_test.rs");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "fn auth_test() {}\n").unwrap();
        let mut resolver = GlobResolver::with_loader_for_tests(dir.path(), counting_loader);

        let matches = resolver.expand("tests/auth_test.rs");

        assert_eq!(matches, [file]);
        assert_eq!(
            CALLS.load(Ordering::SeqCst),
            0,
            "literal existing paths should not invoke the glob loader",
        );
    }

    #[test]
    fn wildcard_pattern_still_uses_loader() {
        static CALLS: AtomicUsize = AtomicUsize::new(0);

        fn counting_loader(pattern: &str, _base_dir: &Path) -> Vec<PathBuf> {
            CALLS.fetch_add(1, Ordering::SeqCst);
            vec![PathBuf::from(pattern)]
        }

        CALLS.store(0, Ordering::SeqCst);
        let dir = TempDir::new().unwrap();
        let mut resolver = GlobResolver::with_loader_for_tests(dir.path(), counting_loader);

        let matches = resolver.expand("tests/**/*.rs");

        assert_eq!(matches, [PathBuf::from("tests/**/*.rs")]);
        assert_eq!(
            CALLS.load(Ordering::SeqCst),
            1,
            "wildcard patterns should keep using the glob loader",
        );
    }

    #[test]
    fn missing_literal_path_bypasses_loader_and_returns_empty() {
        static CALLS: AtomicUsize = AtomicUsize::new(0);

        fn counting_loader(pattern: &str, _base_dir: &Path) -> Vec<PathBuf> {
            CALLS.fetch_add(1, Ordering::SeqCst);
            vec![PathBuf::from(pattern)]
        }

        CALLS.store(0, Ordering::SeqCst);
        let dir = TempDir::new().unwrap();
        let mut resolver = GlobResolver::with_loader_for_tests(dir.path(), counting_loader);

        let matches = resolver.expand("tests/missing.rs");

        assert!(matches.is_empty());
        assert_eq!(
            CALLS.load(Ordering::SeqCst),
            0,
            "missing literal paths should not invoke the glob loader",
        );
    }
}
