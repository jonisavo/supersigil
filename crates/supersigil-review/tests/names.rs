//! Tests typed gate item names, resolution, and refusals.

mod common;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use common::finishes_within;
use supersigil_record::RecordId;
use supersigil_record::reviews::{
    Fingerprint, GateDetail, GateItem, ItemId, Origin, PathBytes, Subject,
};
use supersigil_review::names::{ItemNames, Name, NameError, Parsed, parse};

fn detail(origin: Origin) -> GateDetail {
    match origin {
        Origin::UnexplainedLines => GateDetail::UnexplainedLines {
            unattributed: 1,
            content_match_incomplete: 0,
            unresolved: 0,
            ambiguous: 0,
            line_ending: 0,
            content_match_only: 0,
        },
        Origin::NotDiffed => GateDetail::NotDiffed {
            kind: "binary".to_owned(),
        },
        Origin::UntrackedRecorded => GateDetail::UntrackedRecorded { edits: 1 },
        Origin::NotCaptured => GateDetail::NotCaptured {
            cause: "skip_worktree".to_owned(),
            on_disk: "file".to_owned(),
        },
        Origin::CommitWithoutOrigin => GateDetail::CommitWithoutOrigin,
        Origin::Discontinuity => GateDetail::Discontinuity { gaps: 1 },
        Origin::Unreconciled => GateDetail::Unreconciled {
            reason: "locked".to_owned(),
        },
        Origin::OpenQuestion => GateDetail::OpenQuestion {
            text: "why?".to_owned(),
        },
    }
}

fn item(origin: Origin, subject: Subject) -> GateItem {
    GateItem::new(subject, detail(origin), Fingerprint::default())
}

fn at(origin: Origin, path: &[u8]) -> GateItem {
    item(origin, Subject::Path(PathBytes::new(path)))
}

fn lines(path: &str) -> GateItem {
    at(Origin::UnexplainedLines, path.as_bytes())
}

fn commit(id: &str) -> GateItem {
    item(Origin::CommitWithoutOrigin, Subject::Commit(id.to_owned()))
}

/// Returns a path typed from the repository root, or `None` for non-UTF-8
/// paths or control characters.
fn from_root(path: &PathBytes) -> Option<String> {
    path.to_str()
        .filter(|text| !text.chars().any(char::is_control))
        .map(str::to_owned)
}

fn named(items: &[GateItem], ignore_case: bool) -> ItemNames<'_> {
    ItemNames::new(items, BTreeMap::new(), ignore_case, from_root)
}

fn name(typed: &str) -> Name {
    match parse(typed) {
        Parsed::Path { origin, path } => Name::Path {
            origin,
            path: PathBytes::new(path),
        },
        Parsed::Commit(prefix) => Name::Commit(prefix.to_owned()),
        Parsed::Unreconciled(path) => Name::Unreconciled(PathBuf::from(path)),
        Parsed::Id(prefix) => Name::Id(prefix.to_owned()),
    }
}

fn resolve(
    names: &ItemNames<'_>,
    typed: &[impl AsRef<str>],
) -> Result<Vec<ItemId>, Vec<NameError>> {
    let requests: Vec<(&str, Name)> = typed
        .iter()
        .map(|typed| (typed.as_ref(), name(typed.as_ref())))
        .collect();
    names.resolve(&requests)
}

fn refusal(names: &ItemNames<'_>, typed: &str) -> NameError {
    let mut errors = resolve(names, &[typed]).unwrap_err();
    assert_eq!(errors.len(), 1);
    errors.remove(0)
}

fn ids(items: &[&GateItem]) -> Vec<ItemId> {
    items.iter().map(|item| item.id.clone()).collect()
}

fn strings(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn parsing_splits_a_known_qualifier_from_the_rest() {
    let path = |origin, path| Parsed::Path { origin, path };
    assert_eq!(parse("src/a.rs"), path(None, "src/a.rs"));
    assert_eq!(
        parse("lines:src/a.rs"),
        path(Some(Origin::UnexplainedLines), "src/a.rs")
    );
    assert_eq!(
        parse("notdiffed:a.png"),
        path(Some(Origin::NotDiffed), "a.png")
    );
    assert_eq!(
        parse("untracked:new.rs"),
        path(Some(Origin::UntrackedRecorded), "new.rs")
    );
    assert_eq!(
        parse("uncaptured:c.toml"),
        path(Some(Origin::NotCaptured), "c.toml")
    );
    assert_eq!(
        parse("gap:src/a.rs"),
        path(Some(Origin::Discontinuity), "src/a.rs")
    );
    assert_eq!(parse("commit:abc1234"), Parsed::Commit("abc1234"));
    assert_eq!(
        parse("unreconciled:/work/x"),
        Parsed::Unreconciled("/work/x")
    );
    assert_eq!(parse("id:4f1c9a2e"), Parsed::Id("4f1c9a2e"));
    // Split one known qualifier; keep other colons in the path.
    assert_eq!(
        parse("gap:lines:x"),
        path(Some(Origin::Discontinuity), "lines:x")
    );
    assert_eq!(parse("notes:a.txt"), path(None, "notes:a.txt"));
    assert_eq!(parse(r"C:\work\a.rs"), path(None, r"C:\work\a.rs"));
    assert_eq!(parse("Lines:a.rs"), path(None, "Lines:a.rs"));
}

#[test]
fn a_path_names_its_only_item() {
    let items = [lines("src/a.rs"), at(Origin::NotDiffed, b"logo.png")];
    let names = named(&items, false);
    assert_eq!(names.names(), strings(&["src/a.rs", "logo.png"]));
    assert_eq!(names.name(&items[1].id), Some("logo.png"));
    assert_eq!(resolve(&names, &["src/a.rs"]), Ok(ids(&[&items[0]])));
    // Its qualified name works too.
    assert_eq!(resolve(&names, &["lines:src/a.rs"]), Ok(ids(&[&items[0]])));
}

#[test]
fn every_item_of_a_path_with_several_items_is_qualified() {
    let origins = [
        Origin::UnexplainedLines,
        Origin::NotDiffed,
        Origin::UntrackedRecorded,
        Origin::NotCaptured,
        Origin::Discontinuity,
    ];
    let mut items: Vec<GateItem> = origins.iter().map(|origin| at(*origin, b"p")).collect();
    items.push(lines("q"));
    let names = named(&items, false);
    assert_eq!(
        names.names(),
        strings(&[
            "lines:p",
            "notdiffed:p",
            "untracked:p",
            "uncaptured:p",
            "gap:p",
            "q"
        ])
    );
    for (item, typed) in items.iter().zip(names.names()) {
        assert_eq!(resolve(&names, &[typed]), Ok(ids(&[item])));
    }
}

#[test]
fn a_path_that_gains_a_second_item_needs_its_qualifier() {
    let before = [lines("src/a.rs")];
    let after = [lines("src/a.rs"), at(Origin::Discontinuity, b"src/a.rs")];
    assert_eq!(named(&before, false).names(), strings(&["src/a.rs"]));
    let names = named(&after, false);
    assert_eq!(names.names(), strings(&["lines:src/a.rs", "gap:src/a.rs"]));
    // The qualified name still resolves; the bare name is now ambiguous.
    assert_eq!(
        resolve(&named(&before, false), &["lines:src/a.rs"]),
        Ok(ids(&[&before[0]]))
    );
    assert_eq!(resolve(&names, &["lines:src/a.rs"]), Ok(ids(&[&after[0]])));
    assert_eq!(
        refusal(&names, "src/a.rs"),
        NameError::Several {
            name: "src/a.rs".to_owned(),
            choices: strings(&["lines:src/a.rs", "gap:src/a.rs"]),
        }
    );
}

#[test]
fn names_show_paths_as_typed_from_the_current_directory() {
    // Show paths under `src` as relative and others as absolute.
    let from_src = |path: &PathBytes| {
        from_root(path).map(|text| match text.strip_prefix("src/") {
            Some(below) => below.to_owned(),
            None => format!("/work/repo/{text}"),
        })
    };
    let items = [
        lines("src/a.rs"),
        at(Origin::Discontinuity, b"src/a.rs"),
        at(Origin::NotDiffed, b"logo.png"),
    ];
    let names = ItemNames::new(&items, BTreeMap::new(), false, from_src);
    assert_eq!(
        names.names(),
        strings(&["lines:a.rs", "gap:a.rs", "/work/repo/logo.png"])
    );
}

#[test]
fn a_path_that_reads_as_qualified_is_named_with_its_qualifier() {
    let items = [at(Origin::NotDiffed, b"gap:x")];
    let names = named(&items, false);
    assert_eq!(names.names(), strings(&["notdiffed:gap:x"]));
    assert_eq!(resolve(&names, &["notdiffed:gap:x"]), Ok(ids(&[&items[0]])));
}

#[test]
fn a_commit_is_named_by_its_shortest_unique_prefix_of_at_least_seven() {
    let (first, second, third) = (
        format!("abcdef0123{}", "0".repeat(30)),
        format!("abcdef0129{}", "0".repeat(30)),
        format!("1234567890{}", "0".repeat(30)),
    );
    let items = [commit(&first), commit(&second), commit(&third)];
    let names = named(&items, false);
    assert_eq!(
        names.names(),
        strings(&["commit:abcdef0123", "commit:abcdef0129", "commit:1234567"])
    );
    assert_eq!(resolve(&names, &["commit:1234567"]), Ok(ids(&[&items[2]])));
    assert_eq!(
        resolve(&names, &["commit:ABCDEF0129"]),
        Ok(ids(&[&items[1]]))
    );
    assert_eq!(
        resolve(&names, &[&format!("commit:{first}")]),
        Ok(ids(&[&items[0]]))
    );
    assert_eq!(
        refusal(&names, "commit:abcdef0"),
        NameError::Several {
            name: "commit:abcdef0".to_owned(),
            choices: strings(&["commit:abcdef0123", "commit:abcdef0129"]),
        }
    );
    assert_eq!(
        refusal(&names, "commit:123456"),
        NameError::TooShort {
            name: "commit:123456".to_owned(),
            minimum: 7,
        }
    );
    assert_eq!(
        refusal(&names, "commit:7777777"),
        NameError::NoMatch {
            name: "commit:7777777".to_owned(),
            nearby: Vec::new(),
        }
    );
}

#[test]
fn an_untypeable_path_is_named_by_its_id() {
    let items = [
        at(Origin::NotDiffed, b"bad\xff.bin"),
        at(Origin::NotDiffed, b"esc\x1b[2K.txt"),
    ];
    let names = named(&items, false);
    for (item, typed) in items.iter().zip(names.names()) {
        assert_eq!(typed, &format!("id:{}", &item.id.as_str()[..8]));
        assert_eq!(resolve(&names, &[typed]), Ok(ids(&[item])));
    }
    let full = format!("id:{}", items[0].id);
    assert_eq!(resolve(&names, &[&full]), Ok(ids(&[&items[0]])));
    let short = format!("id:{}", &items[0].id.as_str()[..7]);
    assert_eq!(
        refusal(&names, &short),
        NameError::TooShort {
            name: short.clone(),
            minimum: 8,
        }
    );
}

#[test]
fn two_paths_that_print_alike_get_their_own_names() {
    let (one, two) = (b"a\xff".as_slice(), b"a\xfe".as_slice());
    assert_eq!(String::from_utf8_lossy(one), String::from_utf8_lossy(two));
    let items = [at(Origin::NotDiffed, two), at(Origin::NotDiffed, one)];
    let names = named(&items, false);
    assert_ne!(names.names()[0], names.names()[1]);
    for (item, typed) in items.iter().zip(names.names()) {
        assert!(typed.starts_with("id:"), "{typed}");
        assert_eq!(resolve(&names, &[typed]), Ok(ids(&[item])));
    }
    // Their bytes tell them apart too.
    for (item, bytes) in items.iter().zip([two, one]) {
        let path = Name::Path {
            origin: None,
            path: PathBytes::new(bytes),
        };
        assert_eq!(names.resolve(&[("a?", path)]), Ok(ids(&[item])));
    }
}

#[test]
fn a_case_insensitive_repository_matches_paths_in_any_case() {
    let items = [lines("src/Parser.rs")];
    assert_eq!(
        resolve(&named(&items, true), &["SRC/parser.RS"]),
        Ok(ids(&[&items[0]]))
    );
    assert_eq!(
        refusal(&named(&items, false), "src/parser.rs"),
        NameError::NoMatch {
            name: "src/parser.rs".to_owned(),
            nearby: strings(&["src/Parser.rs"]),
        }
    );
    // Paths that differ only in case are one path when case is ignored.
    let items = [lines("src/A.rs"), at(Origin::Discontinuity, b"src/a.rs")];
    assert_eq!(
        named(&items, true).names(),
        strings(&["lines:src/A.rs", "gap:src/a.rs"])
    );
    assert_eq!(
        named(&items, false).names(),
        strings(&["src/A.rs", "src/a.rs"])
    );
    // Items sharing an origin need IDs to remain distinct.
    let items = [lines("A.rs"), lines("a.rs")];
    let names = named(&items, true);
    for (item, typed) in items.iter().zip(names.names()) {
        assert!(typed.starts_with("id:"), "{typed}");
        assert_eq!(resolve(&names, &[typed]), Ok(ids(&[item])));
    }
}

#[test]
fn unreconciled_names_a_checkout_or_any_association_of_a_record() {
    let record = |id: &str| item(Origin::Unreconciled, Subject::Record(RecordId::new(id)));
    let items = [
        record("rec-2"),
        record("rec-3"),
        item(
            Origin::Unreconciled,
            Subject::Checkout(PathBuf::from("/work/x")),
        ),
    ];
    let associations = BTreeMap::from([(
        RecordId::new("rec-2"),
        vec![PathBuf::from("/work/main"), PathBuf::from("/work/side")],
    )]);
    let names = ItemNames::new(&items, associations, false, from_root);
    // A record without associations is named by its id.
    assert_eq!(
        names.names(),
        strings(&[
            "unreconciled:/work/main",
            &format!("id:{}", &items[1].id.as_str()[..8]),
            "unreconciled:/work/x",
        ])
    );
    assert_eq!(
        resolve(&names, &["unreconciled:/work/side"]),
        Ok(ids(&[&items[0]]))
    );
    assert_eq!(
        resolve(&names, &["unreconciled:/work/x"]),
        Ok(ids(&[&items[2]]))
    );
    assert_eq!(
        refusal(&names, "unreconciled:/work/y"),
        NameError::NoMatch {
            name: "unreconciled:/work/y".to_owned(),
            nearby: Vec::new(),
        }
    );
}

#[test]
fn a_directory_is_refused_with_the_items_under_it() {
    let items = [
        lines("docs/x.md"),
        lines("src/a.rs"),
        at(Origin::NotDiffed, b"src/b/c.png"),
        lines("srcs.rs"),
    ];
    let names = named(&items, false);
    assert_eq!(
        refusal(&names, "src"),
        NameError::Directory {
            name: "src".to_owned(),
            items: strings(&["src/a.rs", "src/b/c.png"]),
        }
    );
    // The repository root holds every path's item.
    let root = Name::Path {
        origin: None,
        path: PathBytes::new(""),
    };
    let NameError::Directory { items: under, .. } =
        names.resolve(&[(".", root)]).unwrap_err().remove(0)
    else {
        panic!("the root is a directory")
    };
    assert_eq!(under.len(), items.len());
}

#[test]
fn a_name_that_matches_nothing_lists_up_to_five_nearby_names() {
    let items = [
        lines("docs/other.md"),
        lines("lib/parsr.rs"),
        lines("src/a.rs"),
        lines("src/b.rs"),
        lines("src/c.rs"),
        lines("src/d.rs"),
        lines("src/e.rs"),
    ];
    let names = named(&items, false);
    assert_eq!(
        refusal(&names, "src/parsr.rs"),
        NameError::NoMatch {
            name: "src/parsr.rs".to_owned(),
            nearby: strings(&[
                "lib/parsr.rs",
                "src/a.rs",
                "src/b.rs",
                "src/c.rs",
                "src/d.rs"
            ]),
        }
    );
    // No item at this path has the requested origin.
    assert_eq!(
        refusal(&names, "gap:docs/other.md"),
        NameError::NoMatch {
            name: "gap:docs/other.md".to_owned(),
            nearby: strings(&["docs/other.md"]),
        }
    );
}

#[test]
fn names_resolve_in_the_order_named_each_item_once() {
    let items = [lines("a.rs"), lines("b.rs")];
    let names = named(&items, false);
    assert_eq!(
        resolve(&names, &["b.rs", "a.rs", "lines:b.rs"]),
        Ok(ids(&[&items[1], &items[0]]))
    );
}

#[test]
fn a_repository_wide_change_is_named_and_resolved_promptly() {
    // After a repository-wide reformat, 5,000 files have unexplained lines;
    // 50 of them also have a gap, and 50 more files cannot be typed.
    let mut items: Vec<GateItem> = (0..5000)
        .map(|k| lines(&format!("src/m{}/f{k}.rs", k % 50)))
        .collect();
    items.extend((0..50).map(|k| {
        at(
            Origin::Discontinuity,
            format!("src/m{k}/f{k}.rs").as_bytes(),
        )
    }));
    items.extend((0..50).map(|k| {
        at(
            Origin::NotDiffed,
            &[format!("bin/{k}").as_bytes(), b"\xff"].concat(),
        )
    }));
    let every = ids(&items.iter().collect::<Vec<_>>());

    // Look up every name by ID, then resolve all names at once.
    let (shown, by_id, resolved) = finishes_within(Duration::from_secs(20), move || {
        let names = named(&items, true);
        let by_id: Vec<String> = items
            .iter()
            .map(|item| names.name(&item.id).unwrap_or_default().to_owned())
            .collect();
        (
            names.names().to_vec(),
            by_id,
            resolve(&names, names.names()),
        )
    });

    assert_eq!(shown.len(), 5100);
    assert_eq!(shown[0], "lines:src/m0/f0.rs");
    assert_eq!(shown[50], "src/m0/f50.rs");
    assert_eq!(shown[5000], "gap:src/m0/f0.rs");
    assert!(shown[5050].starts_with("id:"), "{}", shown[5050]);
    assert_eq!(by_id, shown);
    assert_eq!(resolved, Ok(every));
}

#[test]
fn thousands_of_id_names_resolve_in_one_batch_promptly() {
    let items: Vec<GateItem> = (0..5000).map(|k| lines(&format!("f{k}.rs"))).collect();
    let typed: Vec<String> = items
        .iter()
        .map(|item| format!("id:{}", item.id.as_str()))
        .collect();
    let every = ids(&items.iter().collect::<Vec<_>>());

    let started = std::time::Instant::now();
    let resolved = finishes_within(Duration::from_secs(20), move || {
        let names = named(&items, false);
        resolve(&names, &typed)
    });
    eprintln!("resolved 5,000 id names in {:?}", started.elapsed());

    assert_eq!(resolved, Ok(every));
}

#[test]
fn each_name_that_names_no_single_item_is_refused() {
    let items = [lines("src/a.rs"), at(Origin::Discontinuity, b"src/a.rs")];
    let names = named(&items, false);
    let errors = resolve(&names, &["nope.rs", "lines:src/a.rs", "src", "id:1234"]).unwrap_err();
    assert_eq!(
        errors,
        vec![
            NameError::NoMatch {
                name: "nope.rs".to_owned(),
                nearby: Vec::new(),
            },
            NameError::Directory {
                name: "src".to_owned(),
                items: strings(&["lines:src/a.rs", "gap:src/a.rs"]),
            },
            NameError::TooShort {
                name: "id:1234".to_owned(),
                minimum: 8,
            },
        ]
    );
}

#[test]
fn refusals_say_what_to_name_instead() {
    let message = |error: NameError| error.to_string();
    assert_eq!(
        message(NameError::NoMatch {
            name: "src/parsr.rs".to_owned(),
            nearby: strings(&["src/parser.rs", "lib/parsr.rs"]),
        }),
        "no gate item is named src/parsr.rs; nearby: src/parser.rs, lib/parsr.rs"
    );
    assert_eq!(
        message(NameError::NoMatch {
            name: "nope.rs".to_owned(),
            nearby: Vec::new(),
        }),
        "no gate item is named nope.rs"
    );
    assert_eq!(
        message(NameError::Several {
            name: "src/a.rs".to_owned(),
            choices: strings(&["lines:src/a.rs", "gap:src/a.rs"]),
        }),
        "src/a.rs names 2 gate items; name one of: lines:src/a.rs, gap:src/a.rs"
    );
    assert_eq!(
        message(NameError::Directory {
            name: "src".to_owned(),
            items: strings(&["src/a.rs", "src/b.rs"]),
        }),
        "src is a directory; name each item; there are no wildcards: src/a.rs, src/b.rs"
    );
    assert_eq!(
        message(NameError::TooShort {
            name: "commit:abc".to_owned(),
            minimum: 7,
        }),
        "commit:abc is too short; give at least 7 hex digits"
    );
}
