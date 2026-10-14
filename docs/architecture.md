# Architecture

Supersigil reviews changes using recorded agent activity. It reconstructs
connections between code and observations. It must make development history
easier to inspect without making the evidence look more certain than it is.

The current CLI ingests Claude Code sessions, attributes a selected change,
explains individual lines, and records a person's judgments of a saved
review's gate. Review output is JSON or a plain terminal summary. Recorded
prompts provide conversation context, not an explanation of why an edit was
made.

## Modules

| Crate | Responsibility |
|---|---|
| `supersigil-session` | Discover and parse transcripts; normalize observations; sync them into records. |
| `supersigil-record` | Store versioned observations and derivations; publish and read pinned revisions; keep saved reviews and their judgments in a per-repository review store. |
| `supersigil-git` | Run git; resolve revisions; capture trees; read blobs and converted bytes; identify worktrees and commit origins; keep a judged review's target under a retention ref. |
| `supersigil-anchor` | Validate recorded transformations and combine competing chains into line provenance. |
| `supersigil-review` | Diff blob bytes; turn provenance into review and line-explanation models; build and name gate items; decide their states, carry-forward, and completion from judgments; render summaries. |
| `supersigil` | Select inputs, reconcile records, capture the target, map evidence to worktrees, save reviews, read changed evidence, record judgments, and print results. |

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

Edits come from editing-tool results or one narrow shell form: a top-level
`cat` redirecting a wholly quoted heredoc to a literal path. The call's
harness report must list that file as changed and describe it as created
or modified. Flagged reports confirm nothing because flag meanings are
unestablished. The edit is inferred, not observed: the report confirms the
command changed the file, not that this statement ran. The edit is marked
as read from a shell command; `why` says it was read from a heredoc, never
that the heredoc wrote the line. Every doubt resolves to not recognizing: a
missed write costs attribution, a wrong one creates a false record. A `cd`
applies only within its `&&` chain, which runs only if it succeeds. The
edit names its command and retains the heredoc body without a content hash.
It never borrows report hunks, which cover the whole command. Hunks can only
reject: final-file lines inconsistent with the command's heredocs leave
all writes to that file unrecorded. Command text is read, never run.
Unconfirmed writes count as capture limitations.

Commands retain the harness's report of files differing after the call,
with capped display hunks. This observes changes during the command,
without identifying the statement or bytes responsible. It is never
attribution; hunks are never treated as file bytes. A missing report says
nothing about whether files changed.

Delegation between one session's agents is recorded where it happened.
The transcript that started an agent's work records the start: a dispatch
whose result names the agent, or a resume whose result names the resumed
agent. An edit still in flight when a call starts an agent is not before
it. The same transcript records the earliest notice that the agent
stopped, naming the start. Only records the harness marks as notices
count, each beginning with the notice, since reports can quote any text.
In an agent's transcript, the notice must follow the harness's exact
warning and framing text. Only the notice's header is read: it ends at the
first `<summary>`, `<note>`, or `<result>`, which can quote any text, or
at `</task-notification>`. A header naming its task, call, or status more
than once proves nothing. An earlier queue entry of the same text proves
nothing, since typed messages are queued the same way. Only completed and
failed notices count: an interrupted agent may have had a tool call in
flight. A hand-back message is no such notice, since the agent sends it
before it stops. The agent's own transcript records where it received
work: its dispatch prompt, each resume message, and any later input that
could have woken it, including a record the parser cannot read or place.
Its agent is the one its own session's records name. A record naming
another, of any type, ends its work, and the transcript records no
receive after it. Relayed reports are reports, not a person's messages;
resume messages are delegation. An edit's prompt skips relayed reports.

Observations are append-only. Derivations are rebuildable findings identified
by their inputs and algorithm version. The record format reserves a separate
layer for interpretations, which no shipped workflow writes. A person's
judgments live apart from records, in the review store.

A manifest pins committed log lengths and immutable document names. A reader
uses one manifest and reads only pinned data. A record in another format is
refused, naming its directory; there is no migration. Writers lock the store,
write and synchronize data, then publish the manifest. Unpublished tails and
unreferenced documents do not become part of a revision. A document a later
revision replaced is deleted by the first write more than 14 days after that
revision was published; modification times are never consulted. Only a
derivation document neither the pending manifest nor the published one it
replaces references, under any spelling, and named as the record names its
documents, is ever deleted: any other
file a manifest lists as replaced is dropped from the list and kept. While a
manifest references a file by anything but a plain path inside the record,
through a symbolic link, or a file that cannot be resolved on disk, or while
the manifest, the lock, or the observations or derivations directory is
itself a symbolic link, nothing is deleted.

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
capture. A transcript removed between discovery and read, or no longer a
regular file by then (a FIFO, or a link to a device), is skipped without
being read and reported the same way; opening one never waits for a
writer. Its record keeps the earlier observations, and a later sync
resumes from its cursor if the transcript returns. Other read and sync
failures remain errors.

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

An append adds its text to the previous bytes and reverses only if current
bytes end with that text. Shell edits have no hashes: chains containing
them are exact from the base if base replay reproduces the target,
otherwise consistent. Exact describes bytes, not evidence of statement
execution. A heredoc overwrite with unrecorded previous content stops its
chain, as a Write does.

Time orders presentation, not lineage. Source ordinals order edits within a
transcript. Equal ordinals remain unordered, except one command's heredoc
writes, which are walked in the order the command runs them. Across one
session's transcripts, only recorded delegation orders edits: work an agent
did before its stop precedes what the issuer recorded after the stop, and
what the issuer finished before a start precedes the work it started. A
start is matched with its receipt by the issuer's prompt both record, and a
resume also by its message, never by position; and only when no other
recorded start, in any session, could have sent it. A dispatch's receipt is
proven: its result names the agent. A resume's is inferred, since the
receipt names no sender. Agents work in the prompt that started them, so a
resume is matched only when it comes from a transcript that receives no
work and no other recorded agent could have been working in that prompt by
then: any agent, of any session, that received work there, or work naming
no prompt, could have, unless its dispatch there is matched with a start
the issuer recorded after the resume. An agent's identity is read only from
the harness's structured result, never from text a report could quote.
That no
sender whose records are missing (a transcript not synced, or the part of
one not yet synced) sent the same agent the identical message in the same
prompt is an assumption.
An agent's segment ends at any input that could have woken it. A file's
edits from several transcripts of one session form one sequence only when
every pair is so ordered; otherwise the transcripts stay apart, linked only
by hashes. A session is not ordered when its delegation records contradict
each other, when two sightings of one record differ (a rewritten transcript,
or records that disagree), or when its order would exceed a fixed size
bound. When surviving
chains disagree, report ambiguity. Agreement must include full provenance,
including earlier and whitespace-only contributors.

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
`old_text`, `new_text`, Write content, or a heredoc body. Display-format
changes are a capture limitation that may exclude a competing reading.

Contributors distinguish introduced content, replaced base content,
whitespace-only changes, and earlier participation. Whitespace can change
program behavior, so these changes remain visible. Command change reports
and path mentions are observations, not attribution. Reviews list the
commands reported as changing each file. `why` marks diffs adding the
explained line's text: a display-text match. Reports are capped, so an
absent mark rules nothing out. Recorded commands are never replayed.

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
captured tree. A review does not write the real index, checkout files, or a
ref; only the first judgment on a review writes one. Git filters can have
their own effects, and capture writes git objects.
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

## Saved reviews and judgments

Every review saves its basis, including one run inside an agent session:
the worktree, the base and target trees, the scope, each involved record's
revision with the length and SHA-256 of every pinned log, the unreconciled
checkouts, the derivation algorithm version, a digest of what the review
shows about each reviewed file apart from the commands that mention it,
with every recorded sighting of the edits it refers to, payloads included,
the capture limitations of each candidate transcript as the review saw
them, and the gate. The basis id hashes that content, so an identical
review reuses the saved basis and the judgments on it. A gate item whose
material is unknown salts its basis, so that review is never reused.

A repository has one review store in the private data directory, beside
the records: `--review-dir`, then `SUPERSIGIL_REVIEW_DIR`, then
`$XDG_DATA_HOME/supersigil/reviews`, then `~/.local/share/supersigil/reviews`.
The store names the repository by the canonical path of its git common
directory, which every worktree shares. Writes follow the record protocol:
a writer lock, immutable basis files, an append-only judgment log read only
to its pinned length, and a manifest published by rename. Lock waits are
bounded at 2 seconds: a review whose store stays busy is still printed,
marked unsaved, and judging needs a saved review. A review whose basis
cannot be stored, since it names a path that is not valid UTF-8 (its
worktree's, say) and the store's JSON cannot hold that path, is likewise
printed but never saved. Records hold checkout paths in JSON too: when
reconciling would store a record's checkout that is not valid UTF-8 (a
repository at such a path, reached through a UTF-8 alias that a transcript
recorded, say), the command fails before any review is printed, as it did
before reviews were saved. Bases with no judgment
that no worktree selects leave the manifest 14 days after they were last
reviewed, and their files are deleted a day later, so no published manifest
names a deleted file. Judged bases are kept. Neither store's cleanup ever
deletes a symbolic link or anything through one, and a live basis whose
file is a link is first rewritten as a regular file with the same bytes.
Nor does the review store delete anything while its manifest, lock,
judgments log, or `bases` directory is a link.

The gate holds what the review leaves for a person to consider: per file,
the lines that are unattributed, unresolved, ambiguous, changed only in
line endings, or matched only by content; files not diffed; untracked files
a recorded edit names; paths not captured; commits without a recorded
origin; gaps between recorded edits of a reviewed file; and records or
checkouts not reconciled, including one whose transcript was removed before
it could be read. Coarse diffs, capture limitations, conflicting edits, and
whitespace-only changes remain evidence notes. An item's identity hashes
its origin and its subject (raw path bytes, a commit, a record, or a
checkout), so it is stable across reviews. Its fingerprint hashes, part by
part, the material a judgment of it saw, with every recorded sighting of
the edits behind it, so an edit recorded again under its id with another
payload reopens the item. Material that could not be
established is unknown and matches nothing, not even another unknown, so
such an item is judged again in every review. What is on disk at an
untracked file or an uncaptured path (its kind, executable bit, and bytes
or link target) is read from one entry, never following a link or waiting
for a writer, so a path replaced, or a file whose length, executable bit,
modification time, or (on Unix) status change time changed, while it is
read is unknown, as is a file no longer at the path, unchanged, once read. On Windows
no one can change, move, or delete a file while it is read, and a file
another process holds for writing is unknown. A person
names an item as the summary prints it: by its path, qualified by origin
only when the path has several items. Commits, unreconciled evidence, and
paths that cannot be printed have qualified names of their own. There are
no wildcards.

`judge` records acknowledgments, dismissals with a reason, comments,
completions, and undos against a saved review: the worktree's selection,
which each review run outside an agent session sets, unless `--review`
names another. It never reruns the review or reads the working tree.
Judgments are appended and never edited; an undo withdraws them. Comments
close no item. Completion needs every gate item acknowledged or dismissed,
and records only that the person finished this review: accepting code
remains a git operation. Only a person judges. `judge` refuses to write
while an agent-session marker (`CLAUDECODE`, `CLAUDE_CODE_SESSION_ID`) is
set, and its refusal never suggests unsetting one. Agents may read with
`judge list` and `judge show`.

The first judgment on a review keeps its target under
`refs/supersigil/reviews/<basis id>`: a commit whose tree is the target
tree and whose parents are the base and target commits, created as a
direct ref only when the name is absent and never moved, before the
judgment is appended. A symbolic ref at that name is refused, since what it
keeps would move with the ref it names.
Judging needs the person's git identity, which names who judged and
authors that commit. A review nobody judges writes nothing into the
repository.

Each review relates the judged reviews of the same content (base tree,
target tree, and scope, from any worktree or command) and those whose code
changed (the same worktree, base tree, and scope, another target tree).
An acknowledgment or dismissal carries to the current review when the item
it closed has a matching fingerprint; otherwise the item reopens, naming
the parts that differ. Undoing an item withdraws every judgment that closes
it, own or carried, on every review it closes, and every completion that
counted it.

The review is complete with its own standing completion, or with a standing
completion of the same content whose review held every current item with a
matching fingerprint and against which no evidence changed. Evidence
changed when what the review shows about a reviewed file differs; when an
observation appended to a pinned log since then edits a reviewed path or is
a command in any transcript whose change report lists one, even a command
recorded before without that report; when a capture limitation appended
since then belongs to a candidate transcript; or when a record the earlier
review pinned is gone, one of its logs is shorter, or its pinned prefix
hashes differently. Length alone never establishes an unchanged prefix.
Each current log is
read once per review, however many earlier reviews it is compared with,
and only as the review pinned it: a log that changed since counts as
rewritten.
Commands that only mention a reviewed path, most of them reads, are shown
but never count as changed evidence, and a transcript that became a
candidate since brings none of its earlier capture limitations.
Without a completion that carries, the status reports what changed since
the most recent related completion: the changed paths, the changed evidence
by kind, or the gate items that completion did not cover.

## Limits and product direction

The [product design](research/multiverse-review-design.md) retains the intended
review workflow and decisions for features not yet implemented.

Current attribution is path-based. Renames appear as deletion plus addition.
Scripts, `sed`, formatters, and other file-changing commands appear only
through change reports; their lines stay unattributed. Reports exist from
Claude Code 2.1.272, omit ignored files, and include hunks for at most
five files per command. Recognition stops at statements known to change
what a later one means. That is a list, not a proof against every shell
mechanism; the harness checks stand behind it. Confirmation covers
commands, not statements. If another
statement changes the same file while a heredoc is skipped, and hunks are
absent or show matching final text, the record holds an edit that did not
happen. It attributes only where file bytes reproduce it. Reports describe
final files, so a heredoc overwritten by a later `>` is recorded whether
or not it ran. The created flag describes the pre-command file: an
unrecognized earlier creation makes the inferred absent before-state
wrong, so an append fails its forward check. A `cat` that the user's shell
profile redefines is still read as the program; the hunks reject such a
write only if they show other lines.
Several hashless sessions can remain ambiguous, as can agents of one
session that ran at once. Agents started without a recorded call, such as
by a slash command, stay unordered, as do interrupted agents. Claude Code file-history
backups are not read.

Judgments and their refs stay on this machine; `refs/supersigil/` is not
pushed by default. A moved repository starts a new review store, and the
old one keeps its judgments. The agent check guards against accidents:
anyone can bypass it. An unjudged review's captured tree has no retention
ref and can be pruned by git after 14 days; judging it then needs a new
review. Once a person deletes a judged review's ref and git prunes its
tree, a later review of changed code reports the paths changed since it
as unavailable. Changed evidence ignores conversation turns, and commands
whose change report lists no reviewed path (reads among them), until
annotations use them. A review with an unreconciled record or checkout, or
with an uncaptured path whose on-disk state is a directory (an uncaptured
submodule) or cannot be read, is judged again in every review. A process
holding a review store's manifest for more than a day may find a removed
basis gone, and a reader holding a record's manifest more than 14 days
after a later revision replaced one of its documents may find that
document gone.

Supersigil detects ordinary concurrent writers (an editor, a formatter, or
an agent saving a file while a review reads it) through each file's length,
mode, modification time, and status change time. A process of the same user
that deliberately races Supersigil's system calls, restoring timestamps
within one kernel clock tick, is out of scope: such a process can write the
review store directly. A working tree written while a review reads it can
be read in a state its files never had: git's capture of tracked files and
Supersigil's reads of untracked and uncaptured files take no locks, and a
writer through a shared memory mapping need not update timestamps. Review a
quiet working tree, or review a commit.

The review store and the records are written only by Supersigil. Someone
who edits them by hand (a planted link, FIFO, or file, an edited manifest
or log line) can already make anything appear, so hand edits inside the
stores are handled only where Supersigil's own cleanup would then delete
data a current reference still needs; otherwise a hand-edited store may
make a command fail or wait. Changes outside the stores (the working tree,
git, transcripts, and records as sync writes them) stay in scope.

The intended product adds sourced annotations, durable human corrections,
and a terminal UI with final-change and development-history views. These
are not shipped. Their durable constraints are:

- The host agent runs annotation; Supersigil validates and stores its output.
  Inference stays distinguishable from recorded human and agent statements.
- A changed interpretation is reported apart from changed code and evidence.
- Human corrections survive re-annotation and precede the disputed explanation.

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
  document Bash write recognition and reports.
- [Review outcomes](../crates/supersigil-review/src/outcome.rs) define ordered line rules.
- [Gate items](../crates/supersigil-review/src/gate.rs) and
  [judgment rules](../crates/supersigil-review/src/judging.rs) define item
  identity, fingerprints, carry-forward, and completion; the
  [review store](../crates/supersigil-record/src/reviews.rs) documents its
  write protocol and cleanup.
- [CLI integration tests](../crates/supersigil-cli/tests/cmd_review.rs) and
  [reconciliation tests](../crates/supersigil-cli/tests/reconcile.rs) exercise
  capture, provenance, uncertainty, and record discovery through user workflows.
- [Review scenarios](../crates/supersigil-cli/tests/scenarios.rs) walk
  reviewing, judging, completing, and returning to a review end to end.
