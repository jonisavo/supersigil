# Architecture

Supersigil reviews changes using recorded agent activity. It reconstructs
connections between code and observations. It must make development history
easier to inspect without making the evidence look more certain than it is.

The current CLI ingests Claude Code sessions, attributes a selected change,
and explains individual lines. Review output is JSON or a plain terminal
summary. Recorded prompts provide conversation context, not an explanation
of why an edit was made.

## Modules

| Crate | Responsibility |
|---|---|
| `supersigil-session` | Discover and parse transcripts; normalize observations; sync them into records. |
| `supersigil-record` | Store versioned observations and derivations; publish and read pinned revisions. |
| `supersigil-git` | Run git; resolve revisions; capture trees; read blobs and converted bytes; identify worktrees and commit origins. |
| `supersigil-anchor` | Validate recorded transformations and combine competing chains into line provenance. |
| `supersigil-review` | Diff blob bytes; turn provenance into review and line-explanation models; render summaries. |
| `supersigil` | Select inputs, reconcile records, capture the target, map evidence to worktrees, and print results. |

Git operations belong in `supersigil-git`. Anchor and review computation are
pure over their supplied inputs. They do not discover transcripts, open
stores, or run git. Supersigil runs no LLM and holds no model API key.

## Evidence and records

Conversation history, observed file transformations, and git history are
separate records. Matching content or nearby timestamps do not establish
that they share a lineage.

Observations retain provider identities, transcript ordering, turns, edits,
commands, and capture limitations. An edit distinguishes its operation,
before and after states, retained replacement text, and display patch.
Absent content, unknown content, withheld material, and unavailable material
are different states. A missing transcript does not erase retained history.

An edit comes from an editing tool's result, or from a shell command under
one narrow rule. A command yields an edit only for a top-level `cat`
statement that redirects a wholly quoted heredoc to a literal path, and only
when the harness's change report for that call lists the same file as changed
and describes it as created or modified. A report the harness flagged
confirms nothing: what its flags mean is not established. Such an edit is
inferred, not observed. The report says that the command changed the file,
not that this statement ran, so the edit is marked as read from a shell
command, and `why` says it was read from a heredoc, never that the heredoc
wrote the line. Reading the command resolves every
doubt by not recognizing: a missed write costs attribution, a wrong one would
be a false record. A `cd` counts only for the statements chained to it with
`&&`, which run only when it succeeded. The heredoc body is the edit's text,
and the edit names its command. It carries no content hash and never borrows
the report's hunks, because the report describes the whole command. Those
hunks can only speak against a write: when they show the file, once the
command had finished, holding lines the command's heredocs did not leave
there, the writes to that file are not recorded. Command text is read to find
these statements; it is never run. A write the report does not confirm is
counted as a capture limitation.

A command keeps the harness's change report: the files that differed once
the call had finished, with the harness's capped display hunks. It is an
observation that a file changed while the command ran. It does not say which
statement changed the file or which bytes it wrote, so it is never
attribution, and its hunks are never read as file bytes. A missing report
says nothing about whether files changed.

Observations are append-only. Derivations are rebuildable findings identified
by their inputs and algorithm version. The record format reserves separate
layers for interpretations and human contributions; these are not a shipped
annotation or judgment workflow.

A manifest pins committed log lengths and immutable document names. A reader
uses one manifest and reads only the data it pins. A record in another format
version is refused with its directory named; there is no migration. Writers lock the store,
write and synchronize data, then publish the manifest. Unpublished tails and
unreferenced documents do not become part of a revision.

A review can involve several records. Its output identifies each record and
the revision read. It must not combine observations from different revisions
of the same record.

## Review ordering

The CLI resolves the reviewed worktree, base, and target kind, then finds the
commits and originating worktrees involved. It reconciles relevant records,
including transcripts discovered through checkout associations and existing
cursors, and pins their evidence before capturing a working-tree target.

This ordering matters: Claude Code records a tool result after its file write.
Pinned observations therefore precede the captured working-tree target.
Commit targets do not carry that ordering guarantee.

Reconciliation uses bounded lock waits. Contention is reported as an
unreconciled record or checkout, rather than silently claiming current
capture. Other read and sync failures remain errors.

Deduplicate edit identities across every involved record before filtering to
candidate worktrees or partitioning by path. A conflicting sighting outside
the reviewed worktrees still excludes that edit. Report a conflict on every
candidate path any of its sightings touched.

Map recorded edit paths to the innermost registered worktree containing them.
Do not resolve a symlink within an edit path to attribute an unrelated file.
Candidate transcripts include both transcripts containing candidate edits
and transcripts with observations from candidate worktrees. Fix this set
before attribution. Keep their capture limitations visible even when no edit
is attributed, or every changed line is attributed. When several records hold
one transcript, take its capture limitations from the record whose reports
reach furthest into it. One record's reports cover disjoint ranges; adding
another record's copies would count one problem twice.

## Attribution claims

Anchor walks backward from the target. Each accepted reversal must reproduce
the current bytes when executed forward with the recording tool's semantics.
Every known before and after hash must agree.

| Chain class | Claim |
|---|---|
| Exact from the base | Replaying recorded replacements from the base produces the target. |
| Exact from its start | Recorded hashes verify the chain, but its start is not the base. |
| Consistent | The chain fits the recorded edits but is not verified. |

None of these classes claims capture completeness. An unrecorded excursion
that returns to the same bytes can remain undetectable.

An append executes forward as the previous bytes followed by its text, and
reverses only when the current bytes end with that text. A shell edit has no
hashes, so a chain holding one is exact from the base when replay from the
base reproduces the target, and consistent otherwise. Exact describes the
bytes: it does not add evidence that the heredoc's statement ran. A heredoc
that overwrote a file whose previous content is unrecorded stops its chain,
as a Write does.

Time orders presentation, not lineage. Source ordinals order edits within a
transcript; equal ordinals remain unordered, except the heredoc writes of one
command, which the shell runs in the order the command's text holds them and
which are walked in that order. When surviving chains disagree,
report ambiguity. Agreement must include full provenance, including earlier
and whitespace-only contributors.

A non-base-reaching reading may be set aside only when a base-reaching chain
contains its recorded evidence under the chain comparison rules. Keep it
listed as an alternative assuming an unrecorded change. Never discard a
reading that contains recorded evidence the base-reaching readings lack.

Search, replay, and fallback matching have bounded work budgets. An incomplete
search produces unresolved outcomes, never exact attribution from the subset
explored. Fallback content matches remain labeled matches, not transformation
proof. A partial fallback search must retain its incomplete status: a content
match records whether its search completed, because one found before the
budget ran out may not name every covering edit. Exhausting the fallback
budget never marks the chain search incomplete.

Claude Code's `structuredPatch` is display text, not file bytes. Hunk checks
can reject inconsistent hashless candidates under the supported display
rule; they cannot supply replacement bytes. Exact reversal uses retained
`old_text`, `new_text`, Write content, or a heredoc body. Display-format changes are a capture
limitation, including the possibility of excluding a competing reading.

Contributors distinguish introduced content, replaced base content,
whitespace-only changes, and earlier participation. Whitespace can change
program behavior, so these changes remain visible. Command change reports
and textual command mentions are observations, not attribution. A review
lists the commands reported as changing each file, and `why` marks those
whose reported diff adds the explained line's text; that mark is a match of
display text, and its absence denies nothing, since reports are capped.
Commands recorded in transcripts are never replayed.

## Trees, bytes, and scope

Git locates the repository from its working directory. Every git process
loses inherited variables that name another repository, index, or object
store (`GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`, `GIT_COMMON_DIR`,
`GIT_OBJECT_DIRECTORY`, `GIT_ALTERNATE_OBJECT_DIRECTORIES`) or set how every
pathspec is read (`GIT_LITERAL_PATHSPECS`, `GIT_GLOB_PATHSPECS`,
`GIT_NOGLOB_PATHSPECS`, `GIT_ICASE_PATHSPECS`); Supersigil writes its
pathspecs as literal paths. Replacement refs are disabled, so every object
id in a review names the object stored under it. The user's configuration,
attributes, and filters still apply.

Both review endpoints are complete git trees. Defaults are HEAD for a
working-tree review and the target's first parent for a commit review. Root
commits and unborn HEAD use the empty tree where applicable. An explicit base
is used directly, without an implicit merge-base. Report when it is not an
ancestor of the target.

Working-tree capture uses a temporary index with tracked on-disk changes and
explicitly listed untracked paths. Scope filters narrow the listings, not the
captured tree. Supersigil does not write the real index, checkout files, or a
ref. Git filters can have their own effects, and capture writes git objects.
Unsaved editor buffers are outside the capture.

Tracked changes are included. Untracked files are excluded unless listed,
including files written by recorded edits. List those files with the option
that includes them. Report index flags and other cases where on-disk content
was not captured. List binary, oversized, mode-only, symlink, submodule, and
unsupported changes without pretending they have text attribution.

Diff immutable blob bytes in Rust so user diff configuration cannot change
which lines are reviewed. Attribute using the blob's worktree-form conversion,
since recorded edits describe worktree bytes. Conversion depends on current
attributes and configuration and is evidence of this run. Report conversion
failure and size limits. If converted bytes do not map line for line to blob
bytes, attribution is unavailable. A separate disk comparison may still show
that the captured worktree-form bytes match the file on disk.

Commit origins come from commit-family entries in each worktree's own HEAD
reflog. Missing or unavailable origin evidence remains explicit. Content
matching cannot infer an originating checkout.

`why` attributes its path from the empty tree, even if the file is unchanged
against HEAD. It reports whether the requested line differs from HEAD
separately. It verifies disk capture before reporting a line analysis.

Transcripts are untrusted input. Validate paths against checkout rules and
show terminal control bytes as visible escapes. Preserve the underlying
recorded material.

## Limits and product direction

The [product design](research/multiverse-review-design.md) retains the intended
review workflow and decisions for features not yet implemented.

Current attribution is path-based. Renames appear as deletion plus addition.
Scripts, `sed`, formatters, and every other command that changes files are
visible only through change reports; the lines they wrote stay unattributed.
Change reports exist from Claude Code 2.1.272 on, omit ignored files, and
carry hunks for at most five files of a command. The statements at which the reading of a
command stops are those known to change what a later statement means; that
is a list, not a proof against everything a shell can do, and the harness
checks stand behind it. A shell edit is confirmed
for its command as a whole: when its statement did not run, another
statement changed the same file, and the harness's hunks show nothing that
disagrees (it gave none, or the file ended with the same text), the record
holds an edit that did not happen, which attributes only where the file's
bytes reproduce it. The report describes the file the command ends with, so
a heredoc write that a later `>` of the same command replaced is recorded
whether or not its statement ran. The harness's created flag describes the
file before the command: when an earlier statement that the reader does not
recognize created the file, the edit's absent before-state is wrong, and an
append then fails its forward check. A `cat` that the user's shell profile
redefines is read as the program; the hunks reject such a write only when
they show other lines.
Several hashless sessions can remain ambiguous. Claude Code file-history
backups are not read. Captured trees have no retention ref and can disappear
under git garbage collection. A transcript that vanishes between discovery
and read currently aborts reconciliation.

The intended product adds sourced annotations, durable human corrections,
judgments bound to exact code and evidence revisions, a closing review gate,
and a terminal UI with final-change and development-history views. These are
not shipped. Their durable constraints are:

- The host agent runs annotation; Supersigil validates and stores its output.
  Inference stays distinguishable from recorded human and agent statements.
- A judgment pins the trees, scope, and evidence revisions it addresses.
  Changes to code, interpretation, and relevant evidence are reported separately.
- Human corrections survive re-annotation and precede the disputed explanation.
- Completing a review requires acknowledgment or reasoned dismissal of each
  gate item. Accepting code remains a git operation.
- The first judgment retains its target through a local git ref. Opening a
  review alone does not create that ref.

The record stays local. Capture, model access, and publication need separate
policies, including paths and identifiers. These controls are not implemented.
Publication is an inspected, sanitized export, not replication of private
records. Supersigil can limit what it supplies to the host, but not what the
host can read independently.

## Implementation references

- [CLI pipeline](../crates/supersigil-cli/src/pipeline.rs) and
  [reconciliation](../crates/supersigil-cli/src/reconcile.rs) own input ordering.
- [Anchor steps](../crates/supersigil-anchor/src/step.rs) and
  [chain walk](../crates/supersigil-anchor/src/walk.rs) document algorithm details.
- [Shell write recognition](../crates/supersigil-session/src/claude_code/shell.rs)
  and [change reports](../crates/supersigil-session/src/claude_code/changes.rs)
  document what is read from a Bash call.
- [Review outcomes](../crates/supersigil-review/src/outcome.rs) define ordered line rules.
- [CLI integration tests](../crates/supersigil-cli/tests/cmd_review.rs) and
  [reconciliation tests](../crates/supersigil-cli/tests/reconcile.rs) exercise
  capture, provenance, uncertainty, and record discovery through user workflows.
