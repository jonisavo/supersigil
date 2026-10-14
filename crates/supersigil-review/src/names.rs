//! Item names: the name a person types for each gate item, and how a typed
//! name resolves to items.
//!
//! A path names its only item (`src/parser.rs`); every item of a path with
//! several items needs its origin's qualifier (`lines:src/parser.rs`). A
//! commit is `commit:` with a prefix of its id, evidence not brought up to
//! date is `unreconciled:` with a checkout or any path associated with the
//! record, and any item is `id:` with a prefix of its id, the name of an item
//! whose path cannot be typed. [`parse`] splits a typed name's qualifier from
//! the rest; the CLI turns the rest into a [`Name`], and
//! [`ItemNames::resolve`] finds the items it names. There are no wildcards:
//! a name that matches nothing, several items, or a directory is refused
//! with the names to type instead.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use supersigil_record::RecordId;
use supersigil_record::reviews::{GateItem, ItemId, Origin, PathBytes, Subject};

use crate::summary::printed;

/// Fewest hex digits of a commit id a `commit:` name takes.
const COMMIT_PREFIX: usize = 7;

/// Fewest hex digits of an item id an `id:` name takes.
const ID_PREFIX: usize = 8;

/// Most nearby names a refusal lists.
const NEARBY: usize = 5;

/// Every origin, each with its qualifier.
const ORIGINS: [Origin; 7] = [
    Origin::UnexplainedLines,
    Origin::NotDiffed,
    Origin::UntrackedRecorded,
    Origin::NotCaptured,
    Origin::CommitWithoutOrigin,
    Origin::Discontinuity,
    Origin::Unreconciled,
];

/// A typed name split at its qualifier, before the CLI resolves the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parsed<'a> {
    /// A path as typed, with the origin its qualifier names, if it has one.
    Path {
        /// The origin of `lines:`, `notdiffed:`, `untracked:`,
        /// `uncaptured:`, or `gap:`.
        origin: Option<Origin>,
        /// The path, relative to the current directory unless absolute.
        path: &'a str,
    },
    /// `commit:` and a prefix of the commit's id.
    Commit(&'a str),
    /// `unreconciled:` and a checkout path, or a path associated with a
    /// record.
    Unreconciled(&'a str),
    /// `id:` and a prefix of the item's id.
    Id(&'a str),
}

/// Splits a known qualifier from `typed`. Only one is split, and a name
/// starting with no known qualifier is a path, colons and all.
#[must_use]
pub fn parse(typed: &str) -> Parsed<'_> {
    if let Some(prefix) = typed.strip_prefix("id:") {
        return Parsed::Id(prefix);
    }
    for origin in ORIGINS {
        let Some(rest) = typed
            .strip_prefix(origin.qualifier())
            .and_then(|rest| rest.strip_prefix(':'))
        else {
            continue;
        };
        return match origin {
            Origin::CommitWithoutOrigin => Parsed::Commit(rest),
            Origin::Unreconciled => Parsed::Unreconciled(rest),
            origin => Parsed::Path {
                origin: Some(origin),
                path: rest,
            },
        };
    }
    Parsed::Path {
        origin: None,
        path: typed,
    }
}

/// A typed name with its rest resolved by the CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Name {
    /// A repository path, with the origin its qualifier names, if any. The
    /// empty path is the repository root.
    Path {
        /// The qualified origin.
        origin: Option<Origin>,
        /// The repository path.
        path: PathBytes,
    },
    /// A prefix of a commit's id.
    Commit(String),
    /// A checkout, or a path associated with a record.
    Unreconciled(PathBuf),
    /// A prefix of an item's id.
    Id(String),
}

/// Why a typed name does not name exactly one item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameError {
    /// No item matches.
    NoMatch {
        /// The name as typed.
        name: String,
        /// Up to five names of items with the same file name or in the same
        /// directory.
        nearby: Vec<String>,
    },
    /// Several items match.
    Several {
        /// The name as typed.
        name: String,
        /// Their names.
        choices: Vec<String>,
    },
    /// The name is a directory, and names none of the items under it.
    Directory {
        /// The name as typed.
        name: String,
        /// The names of the items under it.
        items: Vec<String>,
    },
    /// A `commit:` or `id:` prefix is too short.
    TooShort {
        /// The name as typed.
        name: String,
        /// The fewest hex digits it takes.
        minimum: usize,
    },
}

impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoMatch { name, nearby } => {
                write!(f, "no gate item is named {name}")?;
                if !nearby.is_empty() {
                    write!(f, "; nearby: {}", nearby.join(", "))?;
                }
                Ok(())
            }
            Self::Several { name, choices } => write!(
                f,
                "{name} names {} gate items; name one of: {}",
                choices.len(),
                choices.join(", ")
            ),
            Self::Directory { name, items } => write!(
                f,
                "{name} is a directory; name each item; there are no wildcards: {}",
                items.join(", ")
            ),
            Self::TooShort { name, minimum } => {
                write!(f, "{name} is too short; give at least {minimum} hex digits")
            }
        }
    }
}

impl std::error::Error for NameError {}

/// A gate's items with the name a person types for each.
#[derive(Debug, Clone)]
pub struct ItemNames<'g> {
    items: &'g [GateItem],
    associations: BTreeMap<RecordId, Vec<PathBuf>>,
    ignore_case: bool,
    /// The index of every item about a path, in gate order, by the path
    /// as the repository compares paths (see [`folded`]).
    by_path: BTreeMap<Vec<u8>, Vec<usize>>,
    /// Each item's index, by id.
    by_id: BTreeMap<&'g ItemId, usize>,
    /// Every item id with its item's index, sorted, so the ids a prefix
    /// starts form one range.
    id_keys: Vec<(&'g str, usize)>,
    /// The same for the ids of commits.
    commit_keys: Vec<(&'g str, usize)>,
    names: Vec<String>,
}

impl<'g> ItemNames<'g> {
    /// Names `items`. `show` gives a repository path as typed from the
    /// current directory, or `None` when it cannot be typed (it is not
    /// UTF-8 or holds a control character); `associations` are the
    /// checkouts associated with each record not reconciled; with
    /// `ignore_case`, paths that differ only in ASCII case are one path.
    ///
    /// A path names its only item; each item of a path with several is
    /// qualified, and one whose qualified name still matches several items
    /// (paths differing only in case) is named by its id. A commit's name
    /// holds the shortest prefix of at least 7 digits no other commit
    /// shares; an unreconciled item's is its checkout or the record's first
    /// association, when that is typeable and names it alone. Every other
    /// item is `id:` with the shortest prefix of at least 8 digits no other
    /// item shares.
    ///
    /// Items are looked up by path and by id, never compared pairwise, so a
    /// repository-wide change of thousands of files is named promptly.
    pub fn new(
        items: &'g [GateItem],
        associations: BTreeMap<RecordId, Vec<PathBuf>>,
        ignore_case: bool,
        show: impl Fn(&PathBytes) -> Option<String>,
    ) -> Self {
        let mut by_path: BTreeMap<Vec<u8>, Vec<usize>> = BTreeMap::new();
        let mut by_id = BTreeMap::new();
        for (index, item) in items.iter().enumerate() {
            if let Subject::Path(path) = &item.subject {
                let key = folded(path.as_bytes(), ignore_case);
                by_path.entry(key).or_default().push(index);
            }
            by_id.entry(&item.id).or_insert(index);
        }
        let mut names = Self {
            items,
            associations,
            ignore_case,
            by_path,
            by_id,
            id_keys: keyed(items, |item| Some(item.id.as_str())),
            commit_keys: keyed(items, commit_of),
            names: Vec::new(),
        };
        names.names = (0..items.len())
            .map(|index| names.display(index, &show))
            .collect();
        names
    }

    /// Each item's name, in the gate's order.
    #[must_use]
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// The name of the item `id`, if the gate has it.
    #[must_use]
    pub fn name(&self, id: &ItemId) -> Option<&str> {
        let index = self.by_id.get(id)?;
        Some(&self.names[*index])
    }

    /// The item `id`, if the gate has it.
    #[must_use]
    pub fn item(&self, id: &ItemId) -> Option<&'g GateItem> {
        let index = self.by_id.get(id)?;
        Some(&self.items[*index])
    }

    /// The items `names` name, in the order named, each once. Each name
    /// pairs the text typed, which refusals repeat, with what the CLI made
    /// of it.
    ///
    /// # Errors
    ///
    /// Returns a [`NameError`] for each name that does not name exactly one
    /// item, in the order named.
    pub fn resolve(&self, names: &[(&str, Name)]) -> Result<Vec<ItemId>, Vec<NameError>> {
        let mut ids = Vec::new();
        let mut seen = BTreeSet::new();
        let mut errors = Vec::new();
        for (typed, name) in names {
            match self.resolve_one(typed, name) {
                Ok(index) => {
                    let id = &self.items[index].id;
                    if seen.insert(id) {
                        ids.push(id.clone());
                    }
                }
                Err(error) => errors.push(error),
            }
        }
        if errors.is_empty() {
            Ok(ids)
        } else {
            Err(errors)
        }
    }

    /// The index of the one item `name` names.
    fn resolve_one(&self, typed: &str, name: &Name) -> Result<usize, NameError> {
        let matches = match name {
            Name::Path { origin, path } => {
                let matches: Vec<usize> = self
                    .at(path.as_bytes())
                    .iter()
                    .copied()
                    .filter(|index| {
                        origin.is_none_or(|origin| self.items[*index].origin() == origin)
                    })
                    .collect();
                if matches.is_empty() {
                    return Err(self.no_path(typed, path.as_bytes()));
                }
                matches
            }
            Name::Commit(prefix) => by_prefix(typed, prefix, COMMIT_PREFIX, &self.commit_keys)?,
            Name::Id(prefix) => by_prefix(typed, prefix, ID_PREFIX, &self.id_keys)?,
            Name::Unreconciled(path) => self.unreconciled(path),
        };
        match matches.as_slice() {
            [index] => Ok(*index),
            [] => Err(NameError::NoMatch {
                name: typed.to_owned(),
                nearby: Vec::new(),
            }),
            several => Err(NameError::Several {
                name: typed.to_owned(),
                choices: self.names_of(several),
            }),
        }
    }

    /// Why no item is at `path`: a directory holding items, or nothing,
    /// with the items nearby.
    fn no_path(&self, typed: &str, path: &[u8]) -> NameError {
        let under: Vec<usize> = self
            .paths()
            .filter(|(_, other)| self.under(other, path))
            .map(|(index, _)| index)
            .collect();
        if !under.is_empty() {
            return NameError::Directory {
                name: typed.to_owned(),
                items: self.names_of(&under),
            };
        }
        let (directory, file) = split(path);
        let nearby: Vec<usize> = self
            .paths()
            .filter(|(_, other)| {
                let (other_directory, other_file) = split(other);
                self.same(other_file, file) || self.same(other_directory, directory)
            })
            .map(|(index, _)| index)
            .take(NEARBY)
            .collect();
        NameError::NoMatch {
            name: typed.to_owned(),
            nearby: self.names_of(&nearby),
        }
    }

    /// The unreconciled items `path` names: a checkout, or a record it is
    /// associated with.
    fn unreconciled(&self, path: &Path) -> Vec<usize> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| match &item.subject {
                Subject::Checkout(checkout) => checkout == path,
                Subject::Record(record) => self
                    .associations
                    .get(record)
                    .is_some_and(|paths| paths.iter().any(|associated| associated == path)),
                Subject::Path(_) | Subject::Commit(_) => false,
            })
            .map(|(index, _)| index)
            .collect()
    }

    /// The name of item `index`.
    fn display(&self, index: usize, show: &impl Fn(&PathBytes) -> Option<String>) -> String {
        let item = &self.items[index];
        let typed = match &item.subject {
            Subject::Path(path) => show(path).and_then(|shown| self.path_name(index, path, shown)),
            Subject::Commit(commit) => {
                let prefix = unique_prefix(commit, &self.commit_keys, COMMIT_PREFIX);
                Some(format!("commit:{prefix}"))
            }
            Subject::Checkout(checkout) => self.unreconciled_name(index, checkout),
            Subject::Record(record) => self
                .associations
                .get(record)
                .and_then(|paths| paths.first())
                .and_then(|path| self.unreconciled_name(index, path)),
        };
        typed.unwrap_or_else(|| {
            let prefix = unique_prefix(item.id.as_str(), &self.id_keys, ID_PREFIX);
            format!("id:{prefix}")
        })
    }

    /// The name of path item `index` shown as `shown`: bare when it is its
    /// path's only item, qualified when it is its path's only item of its
    /// origin, and none otherwise.
    fn path_name(&self, index: usize, path: &PathBytes, shown: String) -> Option<String> {
        let others: Vec<usize> = self
            .at(path.as_bytes())
            .iter()
            .copied()
            .filter(|other| *other != index)
            .collect();
        // A bare name must not read as qualified.
        if others.is_empty() && matches!(parse(&shown), Parsed::Path { origin: None, .. }) {
            return Some(shown);
        }
        let origin = self.items[index].origin();
        let shared = others
            .iter()
            .any(|other| self.items[*other].origin() == origin);
        (!shared).then(|| format!("{}:{shown}", origin.qualifier()))
    }

    /// `unreconciled:<path>` for item `index`, when the path is typeable
    /// and names that item alone.
    fn unreconciled_name(&self, index: usize, path: &Path) -> Option<String> {
        let text = printed(path.as_os_str())?;
        (self.unreconciled(path) == [index]).then(|| format!("unreconciled:{text}"))
    }

    /// The items about `path`, in gate order, under the repository's case
    /// rule.
    fn at(&self, path: &[u8]) -> &[usize] {
        self.by_path
            .get(&folded(path, self.ignore_case))
            .map_or(&[], Vec::as_slice)
    }

    /// Every item about a repository path, with the path's bytes.
    fn paths(&self) -> impl Iterator<Item = (usize, &[u8])> {
        self.items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| match &item.subject {
                Subject::Path(path) => Some((index, path.as_bytes())),
                _ => None,
            })
    }

    /// Whether `a` and `b` spell one path under the repository's case rule.
    fn same(&self, a: &[u8], b: &[u8]) -> bool {
        if self.ignore_case {
            a.eq_ignore_ascii_case(b)
        } else {
            a == b
        }
    }

    /// Whether `path` lies under `directory`; every path lies under the
    /// repository root, the empty path.
    fn under(&self, path: &[u8], directory: &[u8]) -> bool {
        if directory.is_empty() {
            return true;
        }
        path.get(directory.len()) == Some(&b'/') && self.same(&path[..directory.len()], directory)
    }

    /// The names of the items at `indices`.
    fn names_of(&self, indices: &[usize]) -> Vec<String> {
        indices
            .iter()
            .map(|index| self.names[*index].clone())
            .collect()
    }
}

/// The `key` of each item that has one, with the item's index, sorted.
fn keyed<'g>(
    items: &'g [GateItem],
    key: impl Fn(&'g GateItem) -> Option<&'g str>,
) -> Vec<(&'g str, usize)> {
    let mut keys: Vec<(&str, usize)> = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| key(item).map(|key| (key, index)))
        .collect();
    keys.sort_unstable();
    keys
}

/// The indexes, in gate order, of the items whose key in `keys` (sorted, see
/// [`keyed`]) starts with `prefix`, which must be at least `minimum` digits
/// long. The keys it starts form one range, found by binary search.
fn by_prefix(
    typed: &str,
    prefix: &str,
    minimum: usize,
    keys: &[(&str, usize)],
) -> Result<Vec<usize>, NameError> {
    if prefix.len() < minimum {
        return Err(NameError::TooShort {
            name: typed.to_owned(),
            minimum,
        });
    }
    let prefix = prefix.to_ascii_lowercase();
    let start = keys.partition_point(|(key, _)| *key < prefix.as_str());
    let mut indexes: Vec<usize> = keys[start..]
        .iter()
        .take_while(|(key, _)| key.starts_with(&prefix))
        .map(|(_, index)| *index)
        .collect();
    indexes.sort_unstable();
    Ok(indexes)
}

/// The commit `item` is about, if it is about one.
fn commit_of(item: &GateItem) -> Option<&str> {
    match &item.subject {
        Subject::Commit(commit) => Some(commit),
        _ => None,
    }
}

/// `path` as the repository compares paths: in ASCII lower case when it
/// ignores case, so two spellings of one path are one key.
fn folded(path: &[u8], ignore_case: bool) -> Vec<u8> {
    if ignore_case {
        path.to_ascii_lowercase()
    } else {
        path.to_vec()
    }
}

/// A repository path's directory and file name.
fn split(path: &[u8]) -> (&[u8], &[u8]) {
    match path.iter().rposition(|byte| *byte == b'/') {
        Some(slash) => (&path[..slash], &path[slash + 1..]),
        None => (&[], path),
    }
}

/// The shortest prefix of `id`, at least `minimum` long, that no other key
/// of `keys` (sorted, see [`keyed`]) starts with. The other key sharing the
/// longest prefix with `id` sorts next to it, past any equal to `id`.
fn unique_prefix<'a>(id: &'a str, keys: &[(&str, usize)], minimum: usize) -> &'a str {
    let at = keys.partition_point(|(other, _)| *other < id);
    let before = at.checked_sub(1).map(|before| keys[before].0);
    let after = keys[at..]
        .iter()
        .map(|(other, _)| *other)
        .find(|other| *other != id);
    let length = before
        .into_iter()
        .chain(after)
        .map(|other| common_prefix(id, other) + 1)
        .fold(minimum, usize::max);
    id.get(..length).unwrap_or(id)
}

/// How many leading bytes `a` and `b` share.
fn common_prefix(a: &str, b: &str) -> usize {
    a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count()
}
