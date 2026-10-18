//! Names people type for gate items and rules for resolving them.
//!
//! A path names its only item (`src/parser.rs`). Multiple items at a path
//! need origin qualifiers (`lines:src/parser.rs`). Commits use `commit:` with
//! an ID prefix. Unreconciled evidence uses `unreconciled:` with a checkout
//! or a path associated with the record. An open question uses `question:`
//! with its id (`question:q1`); when another item's question has the same
//! id, the session's shortest unique prefix of at least 8 characters comes
//! before it (`question:3f2a9c1e:q1`). Any item accepts `id:` with an ID
//! prefix, including items whose paths cannot be typed.
//!
//! [`parse`] splits the qualifier; the CLI resolves the rest into a [`Name`];
//! [`ItemNames::resolve`] finds its items. Names have no wildcards. Refusals
//! report missing or ambiguous matches or directories, with alternatives.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use supersigil_record::reviews::{GateItem, ItemId, Origin, PathBytes, Subject};
use supersigil_record::{RecordId, SessionId};

use crate::printed::printed;

/// Minimum hex digits in a `commit:` prefix.
const COMMIT_PREFIX: usize = 7;

/// Minimum hex digits in an `id:` prefix.
const ID_PREFIX: usize = 8;

/// Minimum characters in the session prefix that qualifies a name.
pub(crate) const SESSION_PREFIX: usize = 8;

/// Maximum nearby names in a refusal.
const NEARBY: usize = 5;

/// The origins whose qualifiers [`parse`] reads.
const ORIGINS: [Origin; 8] = [
    Origin::UnexplainedLines,
    Origin::NotDiffed,
    Origin::UntrackedRecorded,
    Origin::NotCaptured,
    Origin::CommitWithoutOrigin,
    Origin::Discontinuity,
    Origin::Unreconciled,
    Origin::OpenQuestion,
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
    /// `question:` and a question's id, with a prefix of its session's id
    /// before it when given.
    Question {
        /// The session prefix: everything before the last colon, since a
        /// question's id holds none.
        session: Option<&'a str>,
        /// The question's id.
        id: &'a str,
    },
}

/// Splits one known qualifier from `typed`. Without a known leading
/// qualifier, treats the whole name as a path, including colons.
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
            Origin::OpenQuestion => match rest.rsplit_once(':') {
                Some((session, id)) => Parsed::Question {
                    session: Some(session),
                    id,
                },
                None => Parsed::Question {
                    session: None,
                    id: rest,
                },
            },
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

/// A parsed name whose remaining text the CLI has resolved.
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
    /// A question's id, and a prefix of its session's id if given.
    Question {
        /// The session prefix.
        session: Option<String>,
        /// The question's id.
        id: String,
    },
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
        /// The minimum number of hex digits.
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
    /// Item indices in gate order, keyed by path under the repository's case
    /// rule (see [`folded`]).
    by_path: BTreeMap<Vec<u8>, Vec<usize>>,
    /// Each item's index, by id.
    by_id: BTreeMap<&'g ItemId, usize>,
    /// Sorted item IDs with their indices, so prefix matches form one range.
    id_keys: Vec<(&'g str, usize)>,
    /// The same for the ids of commits.
    commit_keys: Vec<(&'g str, usize)>,
    /// Each open-question item as (question id, session, item index),
    /// sorted, so the items with one question id form one range, and within
    /// it those whose sessions share a prefix form a sub-range.
    question_index: Vec<(&'g str, &'g str, usize)>,
    /// The sessions of open questions, shortened among themselves.
    question_keys: SessionShorts<'g>,
    names: Vec<String>,
}

/// Sessions shown by their shortest prefix of at least 8 characters unique
/// among them, or the whole id when that is shorter.
#[derive(Debug, Clone)]
pub struct SessionShorts<'a> {
    /// The sessions, sorted, each once.
    sessions: Vec<&'a str>,
}

impl<'a> SessionShorts<'a> {
    /// Shortens among `sessions`, given in any order, each any number of
    /// times.
    #[must_use]
    pub fn new(sessions: impl IntoIterator<Item = &'a SessionId>) -> Self {
        let mut sessions: Vec<&str> = sessions.into_iter().map(SessionId::as_str).collect();
        sessions.sort_unstable();
        sessions.dedup();
        Self { sessions }
    }

    /// `session`'s shortest prefix of at least 8 characters unique among
    /// the sessions, or its whole id when that is shorter. A session not
    /// among them is shortened against them all the same.
    #[must_use]
    pub fn of<'s>(&self, session: &'s SessionId) -> &'s str {
        unique_prefix(session.as_str(), &self.sessions, SESSION_PREFIX)
    }
}

impl<'g> ItemNames<'g> {
    /// Names `items`. `show` returns a repository path as typed from the
    /// current directory, or `None` for non-UTF-8 paths or control characters.
    /// `associations` maps unreconciled records to checkouts. `ignore_case`
    /// treats paths differing only in ASCII case as one path.
    ///
    /// A path names its only item; multiple items require origin qualifiers.
    /// Items whose qualified names remain ambiguous use IDs. Commit names use
    /// the shortest unique prefix of at least 7 digits. Unreconciled items use
    /// the checkout or record's first association if typeable and unambiguous.
    /// An open question uses its id, and when another item's question has
    /// the same id, its session's shortest prefix of at least 8 characters
    /// unique among the questions' sessions, if that name is typeable and
    /// names it alone. Other items use `id:` with the shortest unique prefix
    /// of at least 8 digits.
    ///
    /// Question names depend only on the gate's question items, so `judge`
    /// names them as `review` does without reading records.
    ///
    /// Uses path and ID indexes to avoid pairwise comparisons for changes
    /// across the repository.
    pub fn new(
        items: &'g [GateItem],
        associations: BTreeMap<RecordId, Vec<PathBuf>>,
        ignore_case: bool,
        show: impl Fn(&PathBytes) -> Option<String>,
    ) -> Self {
        let mut by_path: BTreeMap<Vec<u8>, Vec<usize>> = BTreeMap::new();
        let mut by_id = BTreeMap::new();
        let mut question_index = Vec::new();
        for (index, item) in items.iter().enumerate() {
            match &item.subject {
                Subject::Path(path) => {
                    let key = folded(path.as_bytes(), ignore_case);
                    by_path.entry(key).or_default().push(index);
                }
                Subject::Question { session, id } => {
                    question_index.push((id.as_str(), session.as_str(), index));
                }
                Subject::Commit(_) | Subject::Record(_) | Subject::Checkout(_) => {}
            }
            by_id.entry(&item.id).or_insert(index);
        }
        question_index.sort_unstable();
        let question_keys = SessionShorts::new(
            items
                .iter()
                .filter_map(question_of)
                .map(|(session, _)| session),
        );
        let mut names = Self {
            items,
            associations,
            ignore_case,
            by_path,
            by_id,
            id_keys: keyed(items, |item| Some(item.id.as_str())),
            commit_keys: keyed(items, commit_of),
            question_index,
            question_keys,
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

    /// Resolves `names` to items in named order, each once. Each input pairs
    /// the typed text, repeated in refusals, with its CLI-resolved name.
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
            Name::Question { session, id } => self.questions(session.as_deref(), id),
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

    /// Reports `path` as a directory containing items or as a missing match
    /// with nearby items.
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

    /// Finds unreconciled items for checkout `path` or records associated
    /// with it.
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
                Subject::Path(_) | Subject::Commit(_) | Subject::Question { .. } => false,
            })
            .map(|(index, _)| index)
            .collect()
    }

    /// Finds the open-question items with question `id`, in gate order,
    /// whose session starts with `session` when given.
    ///
    /// Reads only the range of `question_index` those items form, so the
    /// work is proportional to the items found, not to all question items.
    fn questions(&self, session: Option<&str>, id: &str) -> Vec<usize> {
        let prefix = session.unwrap_or_default();
        let start = self
            .question_index
            .partition_point(|&(other, session, _)| (other, session) < (id, prefix));
        let mut indices: Vec<usize> = self.question_index[start..]
            .iter()
            .take_while(|&&(other, session, _)| other == id && session.starts_with(prefix))
            .map(|&(_, _, index)| index)
            .collect();
        indices.sort_unstable();
        indices
    }

    /// Whether more than one open-question item has question `id`.
    fn shared_question(&self, id: &str) -> bool {
        let start = self
            .question_index
            .partition_point(|&(other, _, _)| other < id);
        self.question_index[start..]
            .iter()
            .take(2)
            .filter(|&&(other, _, _)| other == id)
            .count()
            > 1
    }

    /// `question:<id>` for item `index`, or `question:<session>:<id>` when
    /// another item's question has the same id, if the name is typeable and
    /// names that item alone.
    fn question_name(&self, index: usize, session: &SessionId, id: &str) -> Option<String> {
        let qualifier = Origin::OpenQuestion.qualifier();
        let prefix = self
            .shared_question(id)
            .then(|| self.question_keys.of(session));
        let name = match prefix {
            Some(prefix) => format!("{qualifier}:{prefix}:{id}"),
            None => format!("{qualifier}:{id}"),
        };
        let typeable = !name.chars().any(char::is_control);
        (typeable && self.questions(prefix, id) == [index]).then_some(name)
    }

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
            Subject::Question { session, id } => self.question_name(index, session, id),
        };
        typed.unwrap_or_else(|| {
            let prefix = unique_prefix(item.id.as_str(), &self.id_keys, ID_PREFIX);
            format!("id:{prefix}")
        })
    }

    /// Names path item `index` as `shown`. Uses a bare name for its path's
    /// only item if it cannot parse as qualified, an origin qualifier if
    /// unique at that path, or `None` if still ambiguous.
    fn path_name(&self, index: usize, path: &PathBytes, shown: String) -> Option<String> {
        let others: Vec<usize> = self
            .at(path.as_bytes())
            .iter()
            .copied()
            .filter(|other| *other != index)
            .collect();
        // Do not let the parser read a bare path as a qualifier.
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

    fn names_of(&self, indices: &[usize]) -> Vec<String> {
        indices
            .iter()
            .map(|index| self.names[*index].clone())
            .collect()
    }
}

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

/// Finds item indices in gate order whose sorted keys ([`keyed`]) start
/// with `prefix`. Requires at least `minimum` digits and finds the matching
/// range by binary search.
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

fn commit_of(item: &GateItem) -> Option<&str> {
    match &item.subject {
        Subject::Commit(commit) => Some(commit),
        _ => None,
    }
}

fn question_of(item: &GateItem) -> Option<(&SessionId, &str)> {
    match &item.subject {
        Subject::Question { session, id } => Some((session, id.as_str())),
        _ => None,
    }
}

/// Builds a path key, folding ASCII case when the repository ignores case.
fn folded(path: &[u8], ignore_case: bool) -> Vec<u8> {
    if ignore_case {
        path.to_ascii_lowercase()
    } else {
        path.to_vec()
    }
}

fn split(path: &[u8]) -> (&[u8], &[u8]) {
    match path.iter().rposition(|byte| *byte == b'/') {
        Some(slash) => (&path[..slash], &path[slash + 1..]),
        None => (&[], path),
    }
}

/// A sorted key a prefix is found among ([`unique_prefix`]): the key
/// itself, or a key with the item index it names ([`keyed`]).
pub(crate) trait Key {
    /// The key.
    fn key(&self) -> &str;
}

impl Key for &str {
    fn key(&self) -> &str {
        self
    }
}

impl Key for (&str, usize) {
    fn key(&self) -> &str {
        self.0
    }
}

/// Finds the shortest unique prefix of `id` with at least `minimum` bytes
/// among sorted `keys`, or all of `id` when it is shorter or the prefix
/// would end inside a character. `id` need not be among `keys`. The
/// longest shared prefix belongs to a neighboring key after skipping keys
/// equal to `id`.
pub(crate) fn unique_prefix<'a>(id: &'a str, keys: &[impl Key], minimum: usize) -> &'a str {
    let at = keys.partition_point(|other| other.key() < id);
    let before = at.checked_sub(1).map(|before| keys[before].key());
    let after = keys[at..].iter().map(Key::key).find(|other| *other != id);
    let length = before
        .into_iter()
        .chain(after)
        .map(|other| common_prefix(id, other) + 1)
        .fold(minimum, usize::max);
    id.get(..length).unwrap_or(id)
}

fn common_prefix(a: &str, b: &str) -> usize {
    a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count()
}
