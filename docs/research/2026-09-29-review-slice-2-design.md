# Review Slice, Plan 2 Design: Snapshots and Attribution

*Design document. 2026-09-29.*

Status: approved by the author on 2026-09-29, section by section. Each section
went through an adversarial Codex review (gpt-6-astra, effort xhigh) until it
approved, before the author saw it: section 1 in four rounds, section 2 in
four, section 3 in seven, section 4 in three, section 5 in five. Claims about
git behavior were verified in scratch repositories on git 2.55, and claims
about Claude Code transcripts were measured over every transcript on the
author's machine, because the reviewer's sandbox could run neither.

Governing spec: `docs/research/multiverse-review-design.md`, sections 3 to 5
and 8, amended the same day where this document says so. Plan 1
(`docs/research/2026-09-28-review-slice-1-record.md`) built the record and
the Claude Code ingestion this design reads.

## Scope and decisions

The plan the plan 1 hand-off called "plan 2" is split in two:

- **Plan 2 (this design): snapshots and attribution.** Git snapshots of a
  base and a target, the reviewed diff, backward chain composition over the
  recorded edits, attribution with named relations, and the `review` and
  `why` commands printing JSON or a plain summary.
- **Plan 3: annotations, judgments, and the gate.** The interpretation and
  contribution layers of the store (typed `put_annotation` and
  `append_judgments`, in the style of `put_derivations` and
  `append_observations`; the raw `put_document` and `append_log` stay
  private), annotation import and the annotate skill, resolution of decision
  anchors, the history perspective, consequences and test freshness, the
  review basis with change detection, gate items, judgments and `judge`, the
  retention ref, and the items parked from plan 1.
- **Plan 4: terminal review.** The ratatui TUI, syntect highlighting, the
  end-to-end slice from design section 8, and the stop hook installed by
  `init`.

The split follows the dependencies: the gate, the basis, and anchor
resolution consume attribution, and attribution consumes nothing from
annotations or judgments. It also puts the least proven part, composing
recorded edits into exact claims, first.

Decisions the author made for plan 2:

1. **Commit targets are attributed.** A worktree originates a reviewed
   commit when its own HEAD reflog has a commit-family entry for it; only
   originating worktrees' edits are candidates (section 2).
2. **Git through the git CLI**, not gix or git2: only git is guaranteed to
   build the same tree `git add` would, under every filter, index, and hook
   configuration.
3. **A fifth library crate, `supersigil-git`**, so `supersigil-anchor` and
   `supersigil-review` stay pure.
4. **The reviewed diff is computed in Rust with `similar`** over blob bytes,
   so user diff configuration cannot change what is attributed.
5. **Tiered claims**, because real transcripts carry no hashes for most
   edits (section 3): "exact" only when hashes or the base bytes verify a
   chain, otherwise "consistent, not verified".
6. **The retention ref moves to plan 3** and is created when the first
   judgment binds to a basis, not on every review open (every sync would
   otherwise mint a new basis and a new ref). Plan 2 writes tree objects
   only.
7. **The design's untracked-file default stays.** Untracked files are
   excluded unless listed; files a recorded edit wrote are listed first with
   the flag that includes them. Including them automatically would make the
   target tree depend on which evidence happens to be recorded.

## Evidence facts

Measured on 2026-09-29 over every Claude Code transcript on the author's
machine (1,058 Edit and 860 Write calls, Claude Code 2.1.258 to 2.1.284).
Design section 4's "Facts about Claude Code transcripts" is amended with
these.

- `originalFile` is present only for small files (at most 8,996 characters
  observed) and null for larger ones (the smallest such file on disk was
  10.7 KB). Plan 1 therefore records the before- and after-states of edits to
  larger files as unknown content: no hashes.
- `structuredPatch` is a display rendering, not bytes: tabs are expanded
  (`\t\t` became four spaces). Reverse-applying hunks literally failed for
  28 of the 151 edits that had `originalFile`, all in tab-indented files.
  The location rule in section 3 (replace the located `newString` with
  `oldString`) reproduced `originalFile` for all 151: 148 had a unique
  occurrence, one needed the hunk's line range to order the occurrences, and
  the two deletions were recovered from their candidate positions, one of
  which jsdiff had displayed a line late because the deleted block began with
  a line identical to the one before it.
- `oldString`, `newString`, and Write `content` are byte-faithful.
- MultiEdit does not occur.
- The display rule is exact: in every hunk of an edit that had
  `originalFile` (274 of 274), the old-side lines (context and `-`) equal the
  original file's lines from `oldStart` with every tab replaced by two
  spaces; neither the raw text nor four-space tabs match the 32
  tab-indented hunks.

Plan 4 therefore renders historical patches from `old_text` and `new_text`,
never from hunks. Anchor uses a hunk only as a consistency check under this
display rule (section 3), never as bytes.

## 1. Crates and data flow

**`supersigil-git`** (new; I/O through the git CLI). Runs git with
`GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`, and `GIT_COMMON_DIR` removed
from the environment and `GIT_OPTIONAL_LOCKS=0` set. Output that carries
paths is read in NUL-delimited formats; object ids are validated as scalars;
blob contents are opaque bytes.

- *Worktrees*: `worktree list --porcelain -z`; the innermost worktree
  containing an absolute path.
- *Snapshots*: a base or commit target resolves to a tree; the working-tree
  target is built in a temporary index updated from the on-disk state of
  tracked files plus listed untracked paths, then `write-tree`. It never
  writes the real index or the worktree.
- *Changes*: `diff-tree -r -z --raw --no-renames`.
- *Bytes*, two kinds kept apart:
  - *blob bytes*: the immutable object contents (`cat-file --batch`). The
    diff is computed on these, so the reviewed change depends only on the
    two trees;
  - *attribution bytes*: the same blob converted to worktree form
    (`cat-file --filters --path`), because the transcript hashes were
    computed over worktree files. Returned with a status: identical to the
    blob bytes, converted, or conversion failed. Conversion depends on the
    current attributes and config, so it is evidence of this review run, not
    a property of the tree.
- *Origins*: the commits in base..target and, per commit, the worktrees whose
  own HEAD reflog has a commit-family entry for it.

**`supersigil-anchor`** (new, pure). Input per path: the base state and the
target state (attribution bytes or absent) and the candidate edits for that
path. It applies its own continuity rules (section 3) rather than reading
the stored discontinuities or restores. Output: backward chain composition
checked at every step, a provenance for every target line, a fate for every
base line, and a labeled content-match fallback. It knows nothing about git
or the store beyond the record's `Edit`, `ContentId`, and related types.

**`supersigil-review`** (new, pure; plan 2 builds only the final-change
part). Diffs blob bytes with `similar`, maps anchor's provenance onto the
diff lines (one to one when the conversion only changed line endings;
otherwise the file's attribution is reported unavailable with that reason),
and builds: spans with named contributors, a status per file, the scope
listing, the origins used and commits with no origin, the capture
limitations of every candidate transcript (including transcripts with no
attributed edit, labeled as not localized to a path), and a prominent
unattributed summary. JSON-serializable. Plan 3 adds the basis, the history
perspective, and the gate on top of the same inputs.

**CLI**: `review` and `why` (section 4). The pipeline:

1. resolve the reviewed worktree, the base, and the target kind; a commit
   target's tree is fixed here because it cannot change;
2. list the commits in base..target (HEAD stands in for a working-tree
   target) and their originating worktrees; the candidate worktrees are
   those plus, for a working-tree target, the reviewed worktree;
3. find the involved records: every record with an association equal to,
   inside, or containing a candidate worktree. A worktree can have several
   (a sibling worktree outside the main checkout gets its own record, and
   `session sync --checkout /repo/src` creates one associated only with that
   subdirectory), and one record can cover several worktrees;
4. reconcile: sync each involved record once per association, with that
   association as the checkout (sync admits only transcripts whose cwd lies
   at or below it), feeding it the transcripts discovered for that
   association plus those the record already holds cursors for whose
   recorded checkout lies under it. Discovery also covers Claude Code project
   directories below each candidate worktree (for example
   `.claude/worktrees/<name>`, or a session started in a subdirectory), and
   the project directories of each ancestor directory of a candidate worktree
   up to and including the root of the repository's main worktree, whose
   encoded names match exactly (a session run in the main checkout can edit
   files inside a nested worktree, and without this a review of the nested
   worktree would never see it unless the main checkout already had a
   record); transcripts found there that no record owns get a record exactly
   as `session sync` would create one, and admission by parsed cwd rejects
   directories whose encoded name only looks nested;
5. pin one `RecordSnapshot` per involved record and read every session's
   observations and derivations from it, keeping each (record id, revision)
   for plan 3's basis;
6. write the working-tree target tree (section 2). It comes after the pin
   because Claude Code logs a tool result only after its write reached the
   disk, so syncing first and snapshotting second guarantees that every
   pinned observation predates a working-tree target, which section 3
   relies on for working-tree targets only;
7. deduplicate every edit of every involved record by event identity
   (section 3) once, across all paths, before anything is partitioned by
   path; then map each accepted edit to the innermost worktree containing
   `checkout/path`; edits outside the candidate worktrees are not
   candidates. A conflicting edit id is excluded from every path and listed on
   every file any of its sightings touched. The candidate
   transcripts, fixed before any attribution runs, are the union of the
   transcripts containing a candidate edit (a session in `/repo` can edit a
   file inside a nested worktree) and the transcripts with an observation
   whose checkout lies in a candidate worktree, whether or not any of their
   edits end up attributed;
8. attribution is requested per path: `review` requests every changed path
   in scope, and anchor runs with that path's base and target attribution
   bytes;
9. build the review model from blob bytes, provenance, and the surrounding
   evidence; print JSON, or a plain text summary until plan 4's TUI replaces
   it.

`why <file>:<line>` shares steps 1 to 7 with the working tree as target,
then requests attribution for its one path directly, whether or not that
file differs from HEAD. It runs anchor with the empty tree as its base, so it
explains a line back to the file's creation when the evidence reaches that
far, and reports separately whether the line differs from HEAD.

**Deferred from plan 2:** `PathLineage` and edit-level `GitAssoc` (design
section 5) have no producer yet. A rename appears as a deletion plus an
addition, and candidate edits are looked up by path only. The
worktree-origin association is computed at review time from reflogs, is not
stored in the record, and never touches the session `DerivationSet` (which
sync recomputes and replaces). The review output lists the associations it
used so plan 3's basis can fingerprint them. Derive keys its groups by the
transcript's cwd, so a session that changes directory edits one file under
two spellings and derive never compares them; re-keying derive to (session,
transcript, normalized absolute path) and re-deriving stored sets whose
algorithm version is stale during reconcile belong to plan 3, where stored
discontinuities and restores are first consumed.

**Record change.** Section 3's reversal rules depend on whether an edit was
an Edit or a Write, and plan 1's `Edit` observation does not keep the tool.
Plan 2 adds a typed `operation` field (`replace` for the Edit tool, `write`
for Write), set by the parser; logs written before it deserialize as
`unknown`, which anchor treats as not reversible (the walk stops with
"operation not recorded").

Dependency direction: git -> (nothing of ours), anchor -> record,
review -> anchor + record, CLI -> all. One new external dependency:
`similar` (Apache-2.0).

## 2. Snapshots, scope, and origins

Everything here lives in `supersigil-git` except edit mapping, which the CLI
does.

**Git itself.** The minimum supported version is 2.36 (`worktree list -z`);
`supersigil-git` checks `git version` once and fails with that minimum named.
Object ids are validated as hex of the repository's object format
(`rev-parse --show-object-format`: 40 or 64 digits). A missing git, a bare
repository, or a directory outside any worktree is an error.

**Base.** A base is either a commit (with its tree) or the empty tree with no
commit.

- `--base <rev>` resolves with
  `rev-parse --verify --end-of-options <rev>^{commit}`; its tree is
  `<commit>^{tree}`.
- Defaults: HEAD for a working-tree target; the first parent for a commit
  target; the empty tree (no commit) for a root commit target or an unborn
  HEAD. The empty tree id comes from `mktree` with empty input, so it is
  right for SHA-1 and SHA-256 repositories.
- The base is used as given, two-dot: no implicit merge-base. When both
  endpoints are commits and the base is not an ancestor of the target commit
  (HEAD for a working-tree target; `merge-base --is-ancestor`), the scope
  listing says the diff includes changes made on the base side. An
  empty-tree base or an unborn HEAD skips the check and reports ancestry as
  unavailable; with an explicit base and no HEAD commit, the review compares
  that base tree with the captured working tree and enumerates no target
  commits.

**Working-tree target.** In the reviewed worktree (`rev-parse
--show-toplevel`), the real index path is resolved first with
`rev-parse --git-path index` and `GIT_INDEX_FILE` scrubbed (that command
honors the variable, so it must not see the override). Every command in
steps 2 to 5 then runs with `GIT_INDEX_FILE` pointing at a temporary index
and with
`-c core.splitIndex=false -c core.fsmonitor=false -c core.hooksPath=<empty temporary directory>`
(the last because writing any index, including an alternate one, runs the
`post-index-change` hook, which could change the worktree or refs; verified
on 2.55, as is the suppression):

1. if the real index file exists, copy it to the temporary path and set the
   copy's modification time to the source's, read from the same open file
   handle. Git decides whether a cached stat entry is racily clean by
   comparing it with the index file's own mtime; a newer copy would let a
   file changed in the same instant as the last index write pass as clean.
   If the real index does not exist (a fresh repository), the temporary path
   is left nonexistent, which git reads as an empty index;
2. `add -u`, which applies clean filters, autocrlf, and LFS exactly as
   `git add` would;
3. `add -- :(literal)<path>` for each path given with `--include-untracked`;
   a listed path that is missing or ignored is an error naming it;
4. `diff-files -z --name-status --ignore-submodules=dirty` against the
   temporary index: every path it still reports is tracked content whose
   on-disk state `add -u` did not capture, whatever the reason, listed as
   "present on disk; contents not captured" or "missing on disk; deletion not
   captured". Verified on 2.55: in a no-cone sparse checkout, a tracked file
   outside the patterns recreated on disk with new contents lost its
   skip-worktree bit, `add -u` kept the old blob, and `diff-files` reported
   it; with a submodule configured `ignore = all`, `add -u` kept an advanced
   submodule's old gitlink and only `--ignore-submodules=dirty` made
   `diff-files` report it;
5. `write-tree`; the result is the target tree.

Writes this can cause, all of them stated in the plan: new objects in the
object database; a refreshed mtime on the shared index file when the copied
index is a split index (git does this on every read of a split index); and
whatever configured clean filters do on their own (for example LFS storing
objects under `.git/lfs`), exactly as `git add` would. It never writes the
real index, the worktree, or any ref. `write-tree` and `read-tree` against
the real index are never used because they can rewrite it (cache-tree).
Before plan 3's retention ref, keeping the written objects is best effort:
they are unreferenced, may be packed, and `gc --prune=now` removes them; a
later reader treats a missing object as a not-reproducible target, not as
corruption.

**Index entries whose on-disk state is not taken.** `diff-files` trusts the
assume-unchanged and skip-worktree flags just as `add -u` does, so flagged
entries are listed from the flags (`ls-files -v -z`), independently of
whether the file exists:

- every assume-unchanged entry, with "missing on disk" or "present on disk;
  contents not captured";
- every skip-worktree entry whose file is present on disk, as "present on
  disk; contents not captured";
- skip-worktree entries whose file is absent, as one count: "N skip-worktree
  paths absent from disk keep their index state";
- paths unmerged in the real index are taken in their on-disk form,
  conflict markers included, and marked unmerged;
- intent-to-add entries are staged like any tracked file (verified on 2.55;
  `git stash create` fails on them).

**Scope defaults** (design section 5, unchanged):

- tracked changes: included;
- untracked, non-ignored files (`ls-files --others --exclude-standard -z`):
  excluded unless given with `--include-untracked`, and listed. Those at a
  path that a candidate edit in the reviewed worktree touched are listed
  first and marked "a recorded edit wrote this file; not included", with the
  flag that includes them; plan 3 can turn that mark into a gate item;
- `-- <path>…` limits the review to literal path prefixes (`:(literal)`
  pathspecs passed to `diff-tree` and `ls-files`), each resolved relative to
  the current directory as git does. A selector that names the repository
  root selects everything: the review then has no path filter, and the scope
  listing says so. `--include-untracked` resolves its paths the same way but
  never treats the root as a selector;
- binary files (a NUL byte in the first 8000 bytes of either side's blob
  bytes, git's heuristic, or either side not valid UTF-8): listed, not
  diffed;
- blobs over 8 MiB: listed as too large to diff;
- mode-only changes (same blob, different mode): listed;
- symlinks (mode 120000), gitlinks (160000), and type changes: listed, not
  diffed, not attributed;
- paths that are not valid UTF-8: listed as unsupported. The raw bytes stay
  the path's identity internally; JSON carries a reversible escaped form
  (backslash and every byte outside printable ASCII escaped as `\xNN`) plus a
  lossy display name.

**Changes.** `diff-tree -r -z --raw --no-renames --no-abbrev
--ignore-submodules=dirty <base tree> <target tree> [-- pathspecs]` gives
status, modes, blob ids, and path for every changed entry. Without the flag,
a gitlink change between two trees is hidden for a submodule configured
`ignore = all` (verified on 2.55).

**Bytes.** Blob bytes for every in-scope side come from one `cat-file
--batch` process. Attribution bytes come from one `cat-file --filters
--path=<path> <blob>` process per text blob: on git 2.55, `--batch --filters`
writes the filtered bytes but reports the unfiltered size in its header, so
batch framing breaks. The status compares the two: identical, converted, or
conversion failed (with the exit status and a stderr tail). Attributes are
read from the reviewed worktree as it is now.

**Origins.**

- Commits: `rev-list <base commit>..<target commit>`, where HEAD stands in
  for a working-tree target. With an empty-tree base, every commit
  reachable from the target commit; with an unborn HEAD, none.
- Worktrees: `worktree list --porcelain -z`. For each one whose directory
  exists, `git -C <worktree> log -g -z --no-abbrev --format=%H%x1f%gs HEAD`
  reads that worktree's own HEAD reflog; one without a reflog contributes
  nothing. Registered worktrees whose directory is missing (`prunable`) are
  listed as "origin evidence unavailable". Record associations that map to
  no registered worktree (a worktree already pruned) are listed the same
  way.
- Grammar of a reflog subject: the action is the text before the first
  `": "` (refnames cannot contain a space, so a refspec such as `main:main`
  stays inside the action), the message is the rest. An entry is an origin
  of its commit when:
  - its action is `commit` or `commit (…)`, or `am`, whatever the message
    (a commit may be titled "Fast-forward handling fix");
  - its action is `cherry-pick` or `revert`, or ends with ` (pick)`,
    ` (reword)`, ` (edit)`, ` (squash)`, ` (fixup)`, ` (continue)`, or
    ` (merge)` whatever precedes it (`rebase`, `rebase -i`,
    `pull --rebase origin main`), and its message is not exactly
    `fast-forward`. The sequencer writes `<action>: fast-forward` when it
    moves HEAD to an existing commit; a commit whose whole title is
    `fast-forward` is indistinguishable and is conservatively not an origin;
  - its action starts with `merge ` or `pull` and its message starts with
    `Merge made by` (fast-forward merges and pulls therefore never match).

  Any other action, including a bare `rebase`, is insufficient evidence:
  never an origin. Git writes these subjects in English regardless of
  locale. Verified on 2.55: `commit`, `commit (initial|amend|merge)`,
  `cherry-pick`, `revert`, `am`, `rebase (pick)`, `merge … Merge made by`,
  `pull <args>: Merge made by`, and `pull --rebase <args> (pick)`; the
  `rebase (merge)` form comes from git's source.
- The output lists, per reviewed commit, its origin worktrees, and lists
  commits with none.

**Edit mapping** (CLI). Worktree roots from `worktree list` and each edit's
`checkout` (stored as the transcript's cwd, not canonical) are both
canonicalized with `supersigil_session::checkout::canonical` when they exist;
when an edit's checkout no longer exists, both sides are compared as written
with the component normalization `checkout::placement` already uses
(separators, drive-letter case). Only directories are canonicalized, never
the edited file, so file symlinks and deleted files cannot change the
result. The edit's already-validated relative path is appended lexically to
the normalized checkout, and the edit maps to the innermost worktree whose
root contains that combined path; its repo-relative path is the combined
path's remainder under that root, joined with `/`. A session in `/repo`
editing `.claude/worktrees/feature/src/lib.rs` therefore maps to the
`feature` worktree as `src/lib.rs`. Lookup against git paths is exact, or
ASCII case-insensitive when the repository sets `core.ignorecase`. An edit
whose checkout cannot be placed is not a candidate and is counted in the
listing.

## 3. Chain composition and attribution

All of this is `supersigil-anchor`.

**Inputs per requested path P.** The base state B and target state T
(attribution bytes, or absent), and the candidate edits mapped to P, already
deduplicated across every path of the review (section 1, step 7); anchor's
deduplication applies the same event-identity rule, which is idempotent on
such input. The rule: the first
occurrence of an edit id within a record wins, as in derive. The same id in
two records with identical payloads is one edit; payloads that differ only
in that one has `operation: unknown` where the other has a known operation
are compatible (the known value is used and both sources are kept, since
sync never rewrites an older log); with any other difference the edit is
excluded and listed as conflicting evidence, never resolved by order. B or T
can instead be *unavailable* (section 2's binary, size, conversion, and
line-structure cases); anchor then returns that status and nothing else.

**Groups and buckets.** A group is the edits of one file (worktree, P) from
one transcript. Within a group, edits with the same `source_ordinal` form an
unordered bucket (one record issued them; nothing says which ran first), and
buckets are ordered by ordinal. A chain that passes through a bucket tries
every order of its edits.

**Reversing a step.** A candidate before-state for edit e on bytes V is
accepted only if all of these hold:

- V hashes to e's after-hash, when it is known;
- the candidate hashes to e's before-hash, when it is known;
- executing e forward on the candidate, with the recorder's semantics
  (`supersigil-session`'s `content.rs`: an ordinary Edit replaces the first
  occurrence of `old_text`, `replace_all` every occurrence; a Write's result
  is its retained content), reproduces V exactly.

Reversal proposes candidates; forward execution validates them, so
reversing a later occurrence of `new_text` that the operation could not have
produced is rejected even when every hash is unknown. Forward execution also
returns the replacements it actually made, each as a pair of half-open byte
ranges (where `old_text` was in the before-state, where `new_text` is in V);
those pairs, not where the reverse proposal put `old_text`, are what
alignment uses. Candidates per operation:

- *Edit* (`operation: replace`): every occurrence of a non-empty `new_text`
  in V; for an empty `new_text` (a deletion, only when `old_text` ends with a
  line terminator, otherwise the step cannot be located) every line start of
  V. Hunk line numbers order the search, nearest first. When the edit's
  patch is retained, its hunks are also a consistency check where no hash
  decides: a candidate before-state is rejected, when e's before-hash is not
  recorded, if its lines at a hunk's `oldStart` differ from that hunk's
  old-side lines under the display rule (tabs as two spaces, terminators and
  a trailing `\r` ignored, `\ No newline` markers skipped), and a step is
  rejected, when e's after-hash is not recorded, if the current bytes differ
  at `newStart` from the hunk's new-side lines. A recorded hash decides on its
  own and the hunk is not consulted. A hunk holding a literal tab cannot come
  from the measured rule, so such an edit's patch is ignored. A display may
  *place* a deletion away from where it happened, but it describes the
  before-file correctly, so the true candidate always passes; a wrong
  insertion position almost never matches the window, which collapses a
  hashless deletion's candidates (every line start) to about one. The check
  only rejects, and every surviving chain is still validated by forward
  execution and hashes or base equality, so it never makes a surviving step
  wrong. It can still hide an alternative: under a display rule that changed
  in a way the tab guard does not see (tabs as four spaces, say), it can
  reject the true candidate of a hashless edit, and when that edit's reading
  competed with another, the survivor's lines look unambiguous. That is the
  stated cost of using the display rule at all; the two guards confine it to
  hashless edits under an undetected change of the rule. With no retained
  patch nothing changes.
- *Edit with `replace_all`*: one candidate, replacing every occurrence of
  `new_text` in V with `old_text` jointly, and only when e's before-hash is
  known: without it, which occurrences of `new_text` existed before the edit
  is not recorded, and several before-states execute forward to the same V.
  When the joint candidate fails, the walk stops with "joint replace-all
  inverse could not be verified".
- *Write creating a file* (before absent): forward execution requires the
  current bytes to equal the retained written content; the before-state is
  then absent. Reaching absence is not by itself a verification.
- *Write overwriting a file*: forward execution requires the current bytes to
  equal the retained written content, and the before-state is known only
  when its before-hash is known and equals B's hash, in which case it is B.
  Otherwise the walk stops with "whole-file write; previous content not
  recorded".
- *Anything else* (`operation: unknown`, text unavailable, nothing
  locatable): the walk stops at e with the reason.

**Alignment** comes from the replacement pairs, never from a diff. Bytes
outside the before ranges are carried unchanged into V, shifted by the
accumulated difference in range lengths, so every unchanged byte has an
exact position on both sides. A line is *kept* when its whole byte span lies
outside the ranges on its side and the image of that span on the other side
is a whole line there (it starts at a line start and ends at the end of a
line); kept lines pair with their images. Every other line belongs to a
changed region: the non-kept lines between two consecutive kept pairs form
one region, before lines on one side and after lines on the other. This
handles splits and joins: for `AxZ\nEND\n` with `x` replaced by `x\n`, `END`
is kept and pairs with the target's `END`, while the target's `Z` line,
whose bytes are carried but begin mid-line in the before-state, joins `Ax`
in the changed region with the before line `AxZ`. Inside a pair, whole lines
common to the start of `old_text` and `new_text` and whole lines common to
their end are kept (models include unchanged context in `old_string`); a
common line counts only when it is whole on both sides. When the common
prefix and suffix overlap (for example `x\nx\n` replaced by `x\n`), the
prefix-first and suffix-first mappings are two candidate alignments, because
equal bytes do not settle which old line survived. The choice is made
independently for each replacement pair of a step (a `replace_all` has one
pair per occurrence), so a step with k such pairs has up to 2^k alignments;
they are generated lazily, one at a time, under the budget below.

**The walk.** Backward from T, over candidate chains:

1. *Heads.* For a working-tree target, the edits of the last bucket of each
   group in the reviewed worktree (every pinned observation predates T, so a
   group's later edits supersede its earlier states), plus any edit of a
   group in an originating worktree (a commit can carry any of its states
   over). For a commit target, any edit of any candidate group. A head is
   tried by reversing it from T.
2. *Predecessors.* At every state a chain has two kinds of candidate
   predecessor, and both are explored: the previous bucket of the group it is
   in (continuing backward without skipping an edit of that group), and every
   *verified link*: an edit of a group the chain has not entered whose
   after-hash is known and equals the hash of the current bytes. A chain
   traverses each group it uses as one contiguous run, may leave a group
   before reaching its first edit, uses each edit at most once, and enters
   each group at most once. There are no links on unknown hashes: with
   nothing recorded to join two transcripts, which one's state the other
   started from is not in the evidence.
3. *Ends.* A chain ends at the first state whose bytes equal B (*from the
   base*), or when no permitted predecessor, local or linked, is accepted; a
   single rejected candidate never ends other continuations. The end and the
   reasons its candidates were rejected are part of the result.

Time never decides. Candidates, bucket orders, and alignment combinations are
generated lazily, one at a time, never materialized as whole sets, and each
is charged against a per-path work budget measured in bytes processed
(64 MiB) before it is explored; every reversal attempted counts, including
failed candidates. The moment the budget runs out, enumeration stops, the
path is marked *search incomplete*, and nothing on it is presented as
exact.

**What a chain establishes.**

- *Exact from the base*: it ends at B. The recorded replacements, applied in
  order at the located positions, transform B into T, with every known hash
  on the way consistent: the design's transformation exactness, established
  by bytes at both ends even where intermediate hashes were never recorded.
- *Exact from its start*: every step has both hashes known and verified, but
  it ends at a state other than B. The transformation from that start to T
  is exact; lines inherited from the start are not attributed (a gap).
- *Consistent*: every step reversed at a candidate position, but some step
  lacks a hash and the chain does not end at B. Reported as "consistent with
  the recorded edits; not verified".

**Evidence alternatives and unrecorded choices.** Heads, verified links, and
different transcripts are *evidence alternatives*: each is a reading of
different recorded events, and they are always kept. A step's replacement
location (several occurrences of `new_text`, the positions of a deletion)
and the order within a bucket are *unrecorded choices*: sibling branches that
differ only in one of them are readings of the same recorded events. Each sibling must pass its own checks;
none is eliminated by another's success. "Exact from the base" never claims
capture completeness, so every exact chain has alternatives that assume some
unrecorded change (a manual swap before a recorded edit, say), and the walk
cannot enumerate them in general. A sibling that does not end at B is exactly
such an alternative: its reading needs an unrecorded state to explain its
start. So a chain that does not end at B is listed on the path as an
*alternative that assumes an unrecorded change*, and is not combined into line
outcomes, when a chain that does end at B entered, in the same order, every
group the first one entered, possibly followed by more, and used, in each,
every edit the first one used: the two differ only in unrecorded choices and
in how far back the recorded evidence reaches, and the first holds no
recorded edit the second lacks. It may use fewer, when a recorded check (an
earlier edit's known after-hash, a creation's content) stopped it early; a
wrong deletion guess on a hashless file is typically stopped exactly there,
and requiring the same set of edits would leave it standing and make every
line ambiguous. The same holds when the base-reaching chain goes on through a
verified link into a group the stopped one never reached. A chain holding an edit
that no base-reaching chain holds is never set aside, so no recorded evidence
is dropped. This is not a claim that they are false; it is the claim every
exact chain makes, applied consistently to the alternatives the location
search happens to generate. When all such siblings end at B, or none does,
they all stay. Evidence alternatives are never set aside by this rule, so a
verified reading from another head or link always keeps its claim.

In practice: one session editing a committed file of any size, with no edits
by other means, has a chain exact from the base when every step is a
supported inversion (not an overwrite whose previous content is not the
base, not a replace-all whose joint inverse fails) and the search completes
within its budget; having an exact chain does not by itself make every line
unambiguous, since another reading that also ends at B (for example two
deletion positions that both replay from B) makes the lines where they
differ ambiguous. Several sessions on a small file are linked by hashes.
Several sessions on a large file yield one consistent chain per session,
which do not compose, so their lines come out ambiguous (explained by one
session's chain, unexplained by another's); that is a stated limitation.

**Combining chains, line by line.** The chains that are not set aside are
alternative histories of the same bytes, not complementary pieces. Each
gives every target line an outcome (a set of origins: base line j,
introduced by e, whitespace added by e; or unexplained, inherited from
before that chain's start) and every removed base line an outcome (a set of
fates: replaced by e, carried to target line k through whitespace-only
edits, removed by a whitespace-only change of e, kept as target line k; or
no fate). For each line:

- every chain gives the same outcome: the line has that outcome, with the
  strongest class among the chains. "The same" compares the full provenance:
  the same origins, the same whitespace-only contributors, and the same
  earlier contributors for a target line, the same fate set for a removed
  base line. Contributors are never merged across chains: two transcripts
  that each record a different edit making the same whitespace change are
  two readings, so the line is ambiguous;
- otherwise the line is *ambiguous*, listing every chain's outcome with its
  class, including "unexplained in chain X". A line explained by one chain
  and unexplained by another is ambiguous, never attributed.

Whether a chain reaches the base is reported per chain, separately from any
line's outcome, since two verified histories can differ only in how much of
the past was captured.

**Line provenance within a chain.** Replay the chain forward from its start
S. During the replay each line carries runs over its non-whitespace
characters, each run with its origin, whitespace-only contributors, and
earlier contributors; lines are reduced to the sets described here only at
the output. Otherwise a whitespace-only join followed by a split would copy
one line's origins onto the other (from `a\nb\n`, E changes `a` to `A`, J
joins the lines, S splits them: ` b\n` must keep base line 2 and never name
E).

- lines of S start as *base line j* (a chain from the base) or *inherited*;
- kept lines keep their provenance;
- a changed region whose removed and added text are equal after deleting all
  whitespace: provenance moves character by character through the
  whitespace-free text, so each character keeps its run's attributes and
  gains a *whitespace-only* contributor e, and each added line's provenance
  is that of exactly the characters it holds. Base-line identity passes through and no *replaced* is
  recorded. A line of such a region with no non-whitespace character has
  nothing to carry: an added one is *whitespace added by e*, a removed one is
  *removed by a whitespace-only change of e*, and neither is ever
  unexplained;
- any other changed region: each added line is *introduced* by e and gains
  an *earlier* contributor for every edit that introduced, changed whitespace
  on, or was earlier on a removed line of that region; each removed base line
  is *replaced* by e.

*Introduced*, *replaced*, and *earlier* are the design's relations.
*Whitespace-only* replaces the design's "rewrote its presentation only": it
is an observed property, not a claim of presentation equivalence, because
removing or adding whitespace can change behavior inside strings or in
indentation-sensitive code. Plan 3 must keep whitespace-only contributions
visible to decision grouping or the gate; it may not drop them as
formatting.

**Content-match fallback.** Reviewed diff lines that no chain explains,
including lines inherited across a gap and paths with no surviving chain,
are checked against whole edits: an edit's non-empty `new_text` (or Write
content) found as an exact byte block in T covers the whole lines it
contains; its `old_text` found in B covers removed lines. A block with no
letter or digit never matches. Each covering edit is listed as introduced or
replaced with evidence *content match*, never *chain*; several stay a list.
The fallback runs only when the search completed, and only for the lines
that need it (a target line with an unexplained origin or no chain, a removed
base line with no fate). Blocks are matched as sequences of whole lines, each
distinct line mapped to an id once and each block's interior lines found with
a linear-time search, the partial first and last lines checked by bytes. The
matching has its own budget (64 MiB, charged per line compared); when it runs
out, the result says the content matching is incomplete, and the lines still
unmatched are reported so.

**Per-path result.** The status (the chains not set aside, with class, head,
end and its reason, and whether from the base; the alternatives that assume
an unrecorded change; *not composed* with reasons; *search incomplete*; or
*unavailable*); edits excluded as conflicting evidence; per target line its
combined outcome with whitespace-only and earlier contributors; per removed
base line its fates; and content matches for the diff lines section 4 asks
about. No class claims capture completeness; section 1 attaches the
candidate transcripts' capture limitations for that.

## 4. Review output, terminal summary, and `why`

The model types live in `supersigil-review` (pure, `serde::Serialize`); the
CLI only gathers inputs and prints.

**Commands.**

```
supersigil review [--base <rev>] [--target <rev> | --working-tree]
                  [--include-untracked <path>]... [--format auto|json|terminal]
                  [--checkout <dir>] [--record-dir <dir>] [--claude-home <dir>]
                  [-- <path>...]
supersigil why <file>:<line> [--format auto|json|terminal]
                  [--checkout <dir>] [--record-dir <dir>] [--claude-home <dir>]
```

- The target defaults to the working tree; `--target` and `--working-tree`
  conflict. `--checkout`, `--record-dir`, and `--claude-home` mean what they
  mean for `session` (plan 1), which is how tests point the commands at
  fixtures.
- `--format auto` (the default for these two commands; plan 1's `session`
  commands keep their defaults) prints the terminal summary when stdout is a
  terminal and JSON otherwise, the behavior the design gives `review`; plan 4
  replaces the terminal summary with the TUI.
- `why` takes a path relative to the current directory and a 1-based line
  number. The line is a line of the captured target (the working-tree
  snapshot), and `why` checks that this is also what is on disk: it reads the
  file and compares it with the attribution bytes; when they differ beyond
  line endings (the path's on-disk state was not captured, or the file
  changed during the command), the result is *not captured* and no line is
  explained. An untracked file is included for `why` without a flag: the
  user named it.
- Reconcile runs before every `review` and `why`. Both locks it can meet are
  bounded: a record's writer lock (for example held by the stop hook) and
  the records-directory lock that record creation and association take
  (plan 1's `open_or_create_record` waits on it without a bound; plan 2 gives
  it a non-blocking attempt). Each is retried for up to two seconds; after
  that the command proceeds without reconciling that record, or without
  creating the record for a checkout whose transcripts were discovered, and
  says so in the output, naming the checkout and the number of transcripts
  when no record id exists yet. Every pinned observation still predates a
  working-tree target, so the result stays sound, only possibly less
  complete.
- Exit status, following plan 1's `main`: 0 whenever a result is produced
  (including an empty or entirely unattributed one), for help and version,
  and when stdout is a closed pipe; 2 for usage errors (clap); 1 for runtime
  errors (not a git worktree, git missing or too old, a bare repository, an
  unresolvable revision, a store error, `why` beyond the end of the file).
  No command in plan 2 fails because of what the review found; judgments and
  the gate are plan 3.

**The reviewed diff.** Per text file in scope, a line diff of the blob bytes
with `similar` (Myers, git's default algorithm), lines split after `\n` and
compared byte for byte, terminators included. The work is bounded
deterministically, never by a clock: common leading and trailing lines are
removed first, and when what remains exceeds 5,000 lines on either side the
file gets one *coarse* hunk that removes every remaining base line and adds
every remaining target line, marked coarse ("too large to align") so no one
reads it as a minimal diff. Hunks carry no context lines; each hunk has its
base and target ranges (1-based start, count) and its removed and added lines
with their terminators (`\n`, `\r\n`, or none for a last line without one),
so a change in line ending or in the final newline is always visible.
Consumers that want context read the blobs by id.

**Mapping attribution onto the diff.** Anchor's provenance is over
attribution bytes, the diff over blob bytes. When both sides' attribution
bytes are identical to their blob bytes, or differ only by line endings (same
line count, each line equal after removing a trailing `\r`), line n maps to
line n. Otherwise the file's attribution is *unavailable: conversion changed
line structure* and no content matching is attempted, since the edit texts
are in worktree form.

Each diff line gets exactly one outer outcome, decided in this order, and
always carries its full provenance (origins or fates, contributors) under
it:

1. the path's search is incomplete: *unresolved (search incomplete)*; the
   surviving chains' outcomes are listed as candidate evidence, never as a
   result;
2. the line is ambiguous: *ambiguous*, with every chain's outcome and class;
3. an added line with any unexplained origin, or a removed line with no
   fate: the content-match fallback runs; covering edits make it *content
   match* (relation, edits, evidence "content match"); otherwise
   *unattributed*, with the reason (no surviving chain, with anchor's status
   and stop reasons; gap before the chain's start; attribution unavailable).
   A line with some origins explained and some not is reported this way too,
   with the explained origins kept in its provenance, so a partly unexplained
   line is never shown as attributed;
4. an added line with an introducing edit among its origins, or a removed
   line with a replacing edit among its fates: *attributed* (the edits, their
   relations, and the classes of the chains);
5. an added line whose origins are all base lines or whitespace added, or a
   removed line whose fates are all carried, kept, or removed by a
   whitespace-only change:
   - with whitespace-only contributors: *whitespace-only change*, by those
     edits, from or to the lines named in the provenance;
   - without, and the corresponding blob lines differ (possible only in line
     ending, since attribution lines map to blob lines after line-ending
     removal): *line ending changed*, which no edit explains and which counts
     in the unattributed summary;
   - without, and the blob lines are byte-identical: *realigned*: every chain
     carries the line unchanged; the diff aligned it differently.

Consecutive diff lines with the same outcome form a *span*; hunks list their
spans in order.

**JSON** (`supersigil.review/1`; a version string, not a promise, while the
pivot is unreleased). Top level, in this order:

- `schema`, `worktree`;
- `base`: `{commit | null, tree, ancestry: ancestor | not_ancestor | unavailable}`;
- `target`: `{kind: working_tree | commit, commit | null, tree}` (for a
  working tree, `commit` is the HEAD it was built over);
- `unattributed`: the prominent summary, first so a reader meets it before
  any detail: counts of unattributed, unresolved, ambiguous, line-ending, and
  content-match-only lines per file; coarse diffs; files whose attribution is
  unavailable; files listed but not diffed (binary, too large, symlinks,
  gitlinks, type changes, unsupported paths); untracked files a recorded edit
  wrote but the scope excluded; paths whose on-disk state was not captured;
- `scope`: the options used and section 2's full listing;
- `origins`: per reviewed commit its origin worktrees, commits without one,
  and worktrees whose origin evidence is unavailable;
- `records`: per involved record its id, pinned revision, and whether it was
  reconciled (with the reason when not);
- `evidence`: the candidate transcripts with their capture limitations
  (marked not localized to a path), unplaced edits (count), discovered
  transcripts not reconciled, and edits excluded as conflicting, each as a
  record-qualified reference (record id, edit id) with the records whose
  payloads disagree; these never enter the `edits` map;
- `files`: one entry per changed path in scope, sorted by path bytes: path
  (and its escaped form when not UTF-8), git status, modes, blob ids,
  attribution-bytes status per side, anchor's status and chains (id, class,
  head, ordered edit ids, end and its reason) and alternatives that assume an
  unrecorded change, hunks with spans, and, for deleted files and files with
  unattributed spans, *mentions*;
- `edits`: every accepted edit referenced anywhere above, keyed by id, with
  session, transcript, turn, time, worktree, `operation`, and *prompt*.

Two fields go beyond attribution and are labeled as observations:

- *mentions*: commands from the candidate transcripts, run in a checkout
  inside the file's worktree, whose command text contains the file's path
  relative to that checkout or absolute. Each carries its command id,
  session, turn, checkout, the command text, and whether a result was
  recorded with its exit status and outcome. Claude Code has no tool that
  deletes files, so a deleted file is otherwise always silent; a mention is
  textual evidence, never attribution, and commands are never replayed.
- *prompt*: for each referenced edit, the nearest recorded ancestor message
  of its turn (following parent pointers within the session) whose role is
  Human or Delegation: its turn id, role, and excerpt with the excerpt's
  material state (retained, withheld, or unavailable); absent when the parent
  chain breaks first. It is the message that preceded the edit in the
  conversation, not a cause or a rationale; interpretations arrive in
  plan 3.

**Terminal summary** (until plan 4). Deterministic plain text, every
untrusted string through plan 1's `format::Untrusted` escaping:

1. a header: worktree, base, target, any record or discovered checkout not
   reconciled, and one line per candidate transcript with capture limitations
   (counts, "not localized to a path"), shown even when every line is
   attributed;
2. the unattributed summary, before anything else, with untracked files that
   a recorded edit wrote listed first, each with the flag that includes it;
3. one line per file: status, path, added and removed line counts, and its
   attribution in words ("12 introduced, 3 replaced, exact from the base, 1
   session"; "2 ambiguous"; "4 unattributed: no surviving chain"; "not
   diffed: binary");
4. scope notes and origin notes, one line each.

No diff text is printed; that is plan 4's job, and `--format json` carries it
now.

**`why`** (`supersigil.why/1`): the path; the target identity (worktree,
target tree, blob id, attribution-bytes status); the same `records` block as
`review` (involved records, pinned revisions, reconcile status,
discovered-but-unreconciled checkouts) and the same `evidence` block's
candidate transcripts with all their capture limitations, unlocalized ones
included, both always present whatever follows; the on-disk check (captured,
or *not captured* with the reason, in which case the line analysis below is
not computed), which compares the file on disk with the target's attribution
bytes (converted when conversion succeeded) independently of whether those
bytes map onto the blob lines, so a filter whose converted bytes cannot be
mapped yields captured with attribution unavailable, never *not captured*;
the line number and text with its terminator; whether the line differs from
HEAD (`yes`, `no`, or `unknown` when the file is untracked, HEAD is unborn,
or the line lies inside a coarse hunk, where the diff does not establish
which lines correspond); anchor's status and chains; the line's outer outcome by the
same ordered rules as a diff line, read against the target line alone (a
line the chains do not explain gets the content-match fallback); and the
`edits` map and conflicting-edit references restricted to the edits this
path's analysis refers to. The terminal form prints the outcome in one or two
sentences, then each contributing edit with its time, session, and prompt
excerpt, then the reconcile and capture-limitation lines the review header
would show.

## 5. Testing and tasks

**Test isolation.** Every test that runs git sets `GIT_CONFIG_NOSYSTEM=1`,
points `HOME` and `XDG_CONFIG_HOME` at its temporary directory, sets author
and committer identity through the environment, and sets `core.autocrlf`
explicitly in each repository (false unless the test is about conversion).
Git for Windows sets `autocrlf` in its system configuration and CI runs on
Linux, Windows, and two macOS runners, so without this the same test would
see different bytes per platform. Tests that need symlinks are `cfg(unix)`.
Non-UTF-8 paths are tested without creating such files: raw `-z` output
fixtures for the parser, and index entries written with
`update-index -z --index-info` (tree objects only, no checkout), which works
on every platform; APFS rejects such file names. The product itself never
sets these variables: it honors the user's configuration.

**Test layers.**

1. *`supersigil-git`*: integration tests against real git in temporary
   repositories, one named test per behavior sections 1 and 2 promise: the
   working-tree tree equals what `add -u` plus the listed paths produce
   (asserted file by file); the real index is byte-identical and keeps its
   mtime; a `post-index-change` hook writing a marker never fires; the copied
   index keeps the source mtime; a fresh repository without an index; unborn
   HEAD with and without `--base`; a SHA-256 repository; a split index;
   assume-unchanged present and missing; a no-cone sparse checkout whose
   recreated file `diff-files` reports; a submodule with `ignore = all`; an
   unmerged path; an intent-to-add entry; `diff-tree -z` parsing; binary,
   size, mode-only, symlink, gitlink, and type-change classification; a
   non-UTF-8 path in a tree built through `update-index --index-info`;
   `cat-file --batch` framing; the conversion status with `autocrlf=true` and
   with an `eol=crlf` attribute; worktree listing, a prunable worktree, and
   innermost-worktree lookup with a nested worktree; reflog origins produced
   by commit, amend, a non-ff merge, a fast-forward merge (no origin),
   cherry-pick, revert, am, rebase pick, `pull --rebase`, a pull merge, a
   commit titled "Fast-forward handling fix", and a linked worktree's own
   reflog. The subject grammar also gets a table-driven unit test over
   literal subjects, including `rebase (merge)` and
   `cherry-pick: fast-forward`.
2. *`supersigil-anchor`*: pure tests over byte strings.
   - One regression test per counterexample raised while this design was
     reviewed: duplicate lines around a replacement; a base state inside a
     segment; a state carried to another worktree mid-segment; equal
     ordinals; a string-literal whitespace change; a formatter join; a
     formatter change to a base line; replayed observations; an internal
     after-hash contradicted by a manual change; two verified histories that
     differ only in which edit produced a state; the forward-execution
     counterexamples for the first occurrence and for `replace_all`; two
     heads that each leave one line unexplained; an overwrite Write whose
     output does not match T; the `foofoo` deletion; a joint `replace_all`
     inverse; a split and a join (`AxZ`); a whitespace-only blank-line
     deletion; the `x\nx\n` prefix/suffix overlap; the history `[a,b]`,
     `a→x`, `b→a`, `x→b`, delete `a` with unknown hashes (both the whole
     sequence and the deletion alone replay from B, so target line `b` is
     ambiguous); a chain with a verified step behind an unknown-hash step
     competing with a verified chain from another head (ambiguous, not set
     aside); the occurrence choice from B=`a\nx\n`, T=`a\na\n` with one
     unknown-hash replacement `x\n`→`a\n` (the reading that edits line 2 ends
     at B and attributes line 2; the other is listed as an alternative that
     assumes an unrecorded change); the same fork where the branches use
     different edits (a verified earlier replacement `q\n`→`a\n` admitted
     only by the line-1 reading: both stay, line 2 ambiguous); and a recorded
     edit reverted by hand before the target (an exact chain is allowed).
   - Corpus-shaped cases: a tab-indented file whose hunks are in display form
     (hunks never used as bytes), a deletion whose hunk sits a line late, a
     large file whose edits carry no hashes (single session: exact from the
     base; with a manual change: consistent), and an `unknown` operation.
   - Unsupported and ambiguous cases, each its own example test: an
     overwrite Write whose previous content is not the base (the walk
     stops), overlapping prefix/suffix alignments (ambiguous lines), a
     `replace_all` whose joint inverse fails, budget exhaustion (search
     incomplete, nothing exact), an unknown-hash deletion with no verified
     chain (ambiguous across its candidate positions), the same deletion with
     a chain from the base (exact; the other positions listed), and a
     multi-line replacement with shared context lines (the context stays base
     lines).
   - Property tests (`proptest`), whose oracle is a separate forward
     interpreter in the test module that shares no code with anchor: it
     applies generated operations to bytes and tags every byte with its
     source (a base offset, or the operation that wrote it).
     (a) *completeness*, over histories restricted to what section 3
     guarantees to invert unambiguously within the budget: one transcript;
     single-line Edit replacements, single-line deletions, and creation
     Writes, where every line an operation introduces is globally fresh
     (never seen in the base or earlier in the history) and old and new lines
     differ in non-whitespace content; hashes randomly known or unknown. With
     globally fresh lines, a step reversed at a wrong position leaves a line
     out of place that no earlier step moves back, so exactly one branch
     reaches B. The walk must find a chain exact from the base, and each
     target line's outcome must match the oracle: base line j when every byte
     carries base tags from base line j, otherwise introduced by the
     operation that wrote the line;
     (b) *soundness*, over the same histories plus one deliberately
     observable interruption: an unrecorded insertion of a fresh, unique
     line that survives to the target. No chain may be exact from the base,
     and that line must never be attributed to a recorded edit. Changes the
     evidence cannot reveal are not asserted on;
     (c) *justification*, over unrestricted generated histories: every
     accepted step executed forward reproduces its after-bytes; every known
     hash on a chain matches; a chain classed exact from the base replays
     from B to T; a branch set aside as assuming an unrecorded change always
     has a chain that ends at B, entered in the same order every group the
     set-aside chain entered (possibly followed by more), and used, in each,
     every edit the set-aside chain used; and a line is attributed only when
     every chain not set aside agrees.
3. *`supersigil-review`*: unit tests for the diff (common-line trimming, the
   5,000-line cutoff and coarse hunk, terminators, final newline), one test
   per ordered outcome rule (including *line ending changed* and
   *realigned*), span grouping; `insta` YAML snapshots of the review JSON and
   the terminal summary for a few constructed inputs.
4. *Record and session*: the parser records `replace` and `write`; an Edit
   observation serialized without the field deserializes as `unknown`; the
   unknown-versus-known deduplication rule (in anchor's tests).
5. *CLI end to end* (`assert_cmd`, temporary repositories, fixture
   transcripts rewritten into the temporary checkout as plan 1's tests do).
   A fixture builder takes explicit initial contents for every file the base
   commit holds (a file deleted by a command, or a large file whose edits
   carry no `originalFile`, has nothing to derive them from), commits them,
   and writes the post-session state into the working tree, plus scripted
   manual changes. Cases:
   - the section 8 slice fixture: the ordinary edit on `src/lib.rs` with its
     manual interruption (exact from its start and a gap; the earlier edit's
     surviving lines stay unattributed, because the later edit rewrote part
     of its block, so no whole block of it occurs in the target); the created
     `src/new.rs` listed as untracked with the recorded-edit mark, and
     attributed when given with `--include-untracked`; `old.txt`, seeded
     explicitly, deleted by `rm` (unattributed, with the command as a
     mention); the `notes.txt` restore (no net change, no diff);
   - a positive content-match case: a gap before a chain whose earlier edit's
     whole `new_text` block survives in the target;
   - a large file edited by one session without `originalFile` (exact from
     the base) and by two sessions (ambiguous lines);
   - a commit target whose commit the test makes with the git CLI in the
     worktree (origin from the reflog), and the same review with the reflog
     expired (commits without origin, unattributed);
   - a sibling worktree with its own record, as origin of a commit reviewed
     from the main worktree; a session in the main checkout editing a file
     inside a nested worktree;
   - `why` on a changed line, on an unchanged line (explained back to the
     file's creation when the fixture creates it), and on an
     assume-unchanged file (*not captured*);
   - records: a record associated only with a subdirectory of the reviewed
     worktree, two records covering one worktree, and a transcript known only
     through a record's cursor (outside every discovery directory); the JSON
     lists each involved record id with its pinned revision, and the cursor
     advances after new lines are appended to that transcript;
   - evidence retention: a candidate transcript with capture limitations and
     no attributed edit appears in the JSON and in the terminal header of a
     review where every line is attributed, and in `why` when its on-disk
     check fails, in both formats;
   - lock contention, one lock at a time with the other free: the test holds
     a record's writer lock and expects that record reported as not
     reconciled; separately it holds the records-directory lock while a new
     checkout's transcripts are discovered and expects the checkout reported
     with its transcript count and no record id. Both exit 0. Timing is
     asserted only by a generous whole-process watchdog (30 seconds), never
     against the two-second retry bound;
   - `--format auto` prints JSON when stdout is not a terminal.
6. *Dogfooding* (the last task, by hand, as in plan 1): run `review` and
   `why` on this repository's own sessions and branches, and write down per
   class how many files and lines came out, which stops occurred, and any
   surprise, for the PR description.

**Tasks.** Each follows TDD and ends with `cargo fmt --all`,
`cargo clippy --workspace --all-targets --all-features`, and
`cargo nextest run`. As in plan 1, each task is one commit, reviewed by Codex
(gpt-6.1-sol) before the next starts; the PR is squash-merged into `pivot` as
PR #14 was.

1. *Record the edit operation*: `operation` on `Edit` (record crate, serde
   default `unknown`), set by the Claude Code parser.
2. *Scaffold the three crates*: `supersigil-git`, `supersigil-anchor`,
   `supersigil-review` as publishable workspace members at 0.14.0 with
   workspace lints, pinned in `[workspace.dependencies]` like plan 1's
   crates; `similar` added (Apache-2.0, on the allow list).
3. *CI coverage*: `ci.yml` runs `cargo nextest run` only on Ubuntu and
   Windows today; macOS jobs compile and lint. Add nextest to both macOS
   runners and print `git --version` in every test job (`ci:` commit type).
4. *Git runner, revisions, worktrees*: the process runner (environment
   scrubbing, `GIT_OPTIONAL_LOCKS=0`, NUL-delimited parsing, version check,
   object format), base and target resolution with the empty tree and
   ancestry, worktree listing and innermost lookup; plus the Linux CI job that
   builds git 2.36.0 from source (cached) and runs the `supersigil-git` tests
   with it on `PATH`, so the stated minimum is exercised rather than assumed.
5. *Working-tree snapshot*: the temporary index procedure and every listing
   attached to it (flagged entries, `diff-files`, unmerged, untracked).
6. *Changes and bytes*: `diff-tree` parsing, scope classification, blob
   bytes, attribution bytes with their status.
7. *Origins*: commits in range, per-worktree reflogs, the subject grammar,
   unavailable origin evidence.
8. *Anchor steps*: candidate generation per operation, forward validation
   returning replacement pairs, alignment (kept lines by byte transport,
   whole-line context, prefix/suffix candidates).
9. *Anchor walk*: deduplication (including unknown versus known operation),
   groups and buckets, heads, local and verified-link predecessors, ends,
   the byte budget, chain classes, and setting aside alternatives that
   assume an unrecorded change.
10. *Anchor provenance*: origins and fates, whitespace-only transport,
    earlier contributors, the per-line combination across chains, the
    content-match fallback, and the property tests.
11. *Review model*: the diff with its cutoff, the outcome rules, spans, the
    JSON model and schema strings, the terminal summary, and the `why` model.
12. *Involved records and bounded reconcile*: enumerating records by
    association (equal, inside, or containing a candidate worktree),
    discovery below candidate worktrees, per-association sync, bounded writer
    and records-directory locks, unreconciled reporting. The records, cursor,
    and lock cases of layer 5 are tested here against the CLI library's
    reconcile function directly, and again end to end in task 13.
13. *`review` and `why` commands*: the pipeline, edit mapping, mentions,
    prompts, formats, exit codes, and the end-to-end tests.
14. *Dogfooding*: the pass described in layer 6. The design doc amendments
    this design implies were made with the design itself.

**Review focus** (inputs a person will hit early, each pinned to a test in
the owning task): an edit to a file over 10 KB (task 10 property tests, task
13 large-file case); a tab-indented file (task 8); a deletion displayed a
line late (task 8); a manual change inside a session (task 13 slice case); a
file the agent created but never staged (task 13); a file deleted with `rm`
(task 13); the stop hook holding the writer lock while `review` runs (task
13); `why` on an assume-unchanged file (task 13); a commit made in a sibling
worktree (task 13); a session started in a subdirectory (task 12); a SHA-256
repository (task 4); git 2.36 (task 4).

## Stated limitations and deferrals

- Path lineage and edit-level git associations: a rename is a deletion plus
  an addition.
- Several sessions on a large file come out ambiguous, because no recorded
  hash joins their transcripts.
- Claude Code's file-history backups (`~/.claude/file-history`) are not
  read; they could supply hashes for large files at checkpoints later.
- Derive's cwd-keyed grouping and re-derivation of stale algorithm versions:
  plan 3.
- The retention ref, the review basis (which will pin a list of (record id,
  revision) pairs, since one review can involve several records), change
  detection, the gate, judgments, and the history perspective: plan 3.
- The TUI and diff text rendering: plan 4.
