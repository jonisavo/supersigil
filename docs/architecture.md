# Architecture

Supersigil reviews changes using recorded agent activity. It reconstructs
connections between code and observations. It must make development history
easier to inspect without making the evidence look more certain than it is.

The current CLI ingests Claude Code sessions, attributes a selected change,
explains individual lines, shows the annotated decisions behind them, and
saves human judgments of review gate items and corrections of annotations.
Review output is JSON or a plain terminal summary. Recorded
prompts provide conversation context, not an explanation of why an edit was
made.

## Modules

| Crate | Responsibility |
|---|---|
| `supersigil-session` | Discover and parse transcripts; normalize observations; sync them into records. |
| `supersigil-record` | Store versioned observations, derivations, and annotation revisions; publish and read pinned revisions; save reviews, judgments, and corrections per repository. |
| `supersigil-git` | Run git; resolve revisions; capture trees; read blobs and converted bytes; identify worktrees and commit origins; retain judged target trees under git refs. |
| `supersigil-anchor` | Validate recorded transformations and combine competing chains into line provenance. |
| `supersigil-review` | Diff blob bytes; turn provenance into review and line-explanation models; validate annotations; attach decisions to the lines they explain and anchor corrections to them; name gate items; apply judgments and carry them to later reviews; determine completion; render summaries. |
| `supersigil` | Select inputs, reconcile records, capture the target, map evidence to worktrees, prepare and import annotations (`annotate`), save reviews, read changed evidence and annotations, record judgments and corrections, and print results. |

Git operations belong in `supersigil-git`. Anchor and review computation are
pure over their supplied inputs. They do not discover transcripts, open
stores, or run git. Supersigil runs no LLM and holds no model API key.

## Evidence and records

Conversation history, observed file transformations, and git history are
separate records. Matching content or nearby timestamps do not establish
that they share a lineage.

Observations retain provider identities, transcript ordering, turns, edits,
commands, and capture limitations. Human, agent, delegation, and report
turns retain their text whole. Thinking is never read; tool results,
harness metadata, and compaction summaries are unavailable. An edit
distinguishes its operation, before and after states, retained replacement
text, and display patch. Absent content, unknown content, withheld
material, and unavailable material are different states. A missing
transcript does not erase retained history.

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
by their inputs and algorithm version. Interpretations are a separate layer:
one immutable document per annotation revision of a session, listed by the
manifest and never cleaned up. Only `annotate import` writes them. Human
judgments, corrections included, live in the review store, separate from
records.

A manifest pins committed log lengths and immutable document names. A reader
uses one manifest and reads only pinned data. A record in another format is
refused, naming its directory; there is no migration. Writers lock the store,
write and synchronize data, then publish the manifest. Unpublished tails and
unreferenced documents do not become part of a revision. Cleanup first tries
to delete a replaced document on a write more than 14 days after the
replacement revision, without consulting modification times. Failed deletions
are retried on later writes. Cleanup deletes only derivation documents with the
record's filename format that neither the pending nor the previous published
manifest references, under any spelling. It keeps other files listed as
replaced and removes them from that list. Cleanup deletes nothing if either
manifest references a file through a symlink, by a path outside the record
or not in plain form, or by a path that cannot be resolved. It also deletes
nothing if the manifest, lock, observations directory, or derivations
directory is a symlink.

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
capture. Reconciliation also skips and reports transcripts that disappear
or become non-regular files between discovery and read, such as FIFOs or
links to devices. Opening a transcript never waits for a writer. The record
keeps its observations and cursor; sync resumes there if the transcript
returns and its consumed bytes still match. Other read and sync failures
remain errors.

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

Every review, including one run by an agent, attempts to save a basis containing:

- The worktree, base and target trees, and scope.
- Each record's revision and each pinned log's length and SHA-256.
- Unreconciled checkouts and the derivation algorithm version.
- A digest of each reviewed file's output, excluding commands that mention
  it, and every recorded sighting of its referenced edits, including payloads.
- Each candidate transcript's capture limitations and the gate.
- Each candidate session's annotation pin and the decisions in view.

The basis id hashes this content. Identical reviews reuse the saved basis
and its judgments. Unknown gate material adds a salt, preventing reuse.

Each repository has a review store in the private data directory beside the
records. Its root comes from
`--review-dir`, then `SUPERSIGIL_REVIEW_DIR`, then
`$XDG_DATA_HOME/supersigil/reviews`, then `~/.local/share/supersigil/reviews`.
The repository key is its canonical git common directory, shared by all
worktrees. Writers lock `write.lock`, write immutable `bases/<id>.json`
files, append to `judgments.jsonl`, then publish `manifest.json` by rename.
Readers stop at the manifest's pinned judgment-log length. A review store in
another format is refused, naming its directory.

Lock waits stop after 2 seconds. A busy store leaves the review printed but
unsaved; judging requires a saved basis. A basis containing a non-UTF-8 path
is also printed but unsaved because JSON cannot store that path. Records
also store checkout paths as JSON. If reconciliation must store a non-UTF-8
checkout path, the command fails before printing the review, even if the
transcript reached the checkout through a UTF-8 alias.

Bases with no judgment and no worktree selection leave the manifest 14 days
after their last review. Cleanup deletes their files a day later, so no
published manifest names a deleted file. Judged bases stay. Neither store's
cleanup deletes symlinks or files through them. A live basis file that is a
symlink is first rewritten as a regular file with the same bytes. Review
cleanup deletes nothing if the manifest, lock, judgment log, or `bases`
directory is a symlink.

The gate lists items for a person to consider:

- Per file, lines that are unattributed, unresolved, ambiguous, changed only
  in line endings, or matched only by content.
- Files not diffed, untracked files named by recorded edits, and uncaptured paths.
- Commits without recorded origins and gaps between recorded edits of a reviewed file.
- Unreconciled records and checkouts, including transcripts removed before reading.
- Open questions of the annotations read, about the change.

Coarse diffs, capture limitations, conflicting edits, and whitespace-only
changes remain evidence notes. An item's id hashes its origin and subject
(raw path bytes, commit, record, or checkout), so it stays stable across
reviews. Its fingerprint hashes the judged material in parts, including
every sighting of its referenced edits. A new payload recorded under an
existing edit id therefore reopens the item. Unknown material matches
nothing, including another unknown; each review requires a new judgment.

For untracked files and uncaptured paths, Supersigil reads one entry's kind,
executable bit, and bytes or link target without following links or waiting
for a writer. The state is unknown if the path is replaced during the read,
or if the file's length, executable bit, modification time, or Unix status
change time changes. It is also unknown if the unchanged file is no longer
at the path after reading. Windows blocks changes, moves, and deletion
while the file is read; a file held for writing by another process is unknown.

Use the item name printed in the summary. Paths gain an origin qualifier
only when they have several items. Commits, unreconciled evidence, and
unprintable paths have their own qualified names. There are no wildcards.

`judge` records acknowledgments, dismissals with a reason, comments,
corrections, completions, and undos on a saved basis. Each review outside an
agent session selects its basis for the worktree; `--review` overrides that
selection.
`judge` never reruns the review or reads the working tree. Judgments are
append-only; an undo withdraws them. A reason or text, when given, is never
blank. Comments close no item. Completion requires every gate item
acknowledged or dismissed. It records that the person finished reviewing;
accepting code remains a git operation.

`judge` refuses to write while either agent marker, `CLAUDECODE` or
`CLAUDE_CODE_SESSION_ID`, is set and non-empty. Its refusal never suggests
unsetting a marker. Agents may run `judge list` and `judge show`.

Before appending the first judgment, Supersigil retains the target under
`refs/supersigil/reviews/<basis id>`. This direct ref points to a commit with
the target tree and the base and target commits as parents, where present,
in that order without duplicates.
It is created only if absent and never moved. A symbolic ref is refused
because its target could move. Judging requires a git identity to name the
person and author the retention commit. An unjudged review creates no
retention ref.

Related judged reviews either share the base tree, target tree, and scope
(across worktrees and commands), or share the worktree, base tree, and scope
with a different target tree. An acknowledgment or dismissal carries to
an item with the same id and a matching fingerprint. Otherwise the item
reopens and lists the differing fingerprint parts. A standing completion
counts the judgments it lists as closing their items on its own review, so
a judgment carried into a completed review carries on from it, still under
its original id. A completion that carries therefore never leaves an item
open. Undoing an item withdraws
every judgment closing it, including carried judgments, on every review
those judgments close. It also withdraws every completion that counted them.
Undoing a judgment by id withdraws what stands of it, with the completions
that counted it; the judgment must be recorded on the selected review or
close one of its items.

A review is complete with its own standing completion. A completion from a
review of the same content carries only if it covered every current item
with a matching fingerprint and no evidence changed against that review.
Either counts only while the judgment it lists for each current item still
stands for that item. Undo keeps this true by withdrawing the completions
that count what it withdraws; a completion that would leave an item open is
never counted, and a new one may be recorded once the items close.
Evidence changes include:

- Different output about a reviewed file.
- Appended observations that edit a reviewed path, or commands whose change
  reports list one, in any transcript. A new change report counts even if
  the command was already recorded without it.
- Appended capture limitations from a candidate transcript.
- A missing pinned record, a shortened log, or a changed pinned-prefix hash.

Log length alone cannot prove the prefix unchanged. Supersigil reads each
current log once per review, regardless of the number of related reviews,
and only as pinned by the current review. A log changed since that pin
counts as rewritten. Commands that only mention reviewed paths, usually
reads, are shown but do not count as changed evidence. A newly candidate
transcript contributes none of its earlier capture limitations.

If no completion carries, the status reports changes since the most recent
related completion: changed paths, changed evidence by kind, or gate items
that completion did not cover.

## Interpretations and annotations

An annotation interprets one session: the decisions it made, each citing the
edits and commands that carried it out, with rationale, alternatives,
implications, and open questions. Each rationale line names its speaker:
`human`, `agent`, or `annotator` for an inference. An explicit line quotes
the turn that states it. The host agent writes annotations; Supersigil
validates and stores them, and runs no model.

`annotate prepare <session>` reconciles the checkout's records as a review
from the checkout does, with the usual bounded wait, and finds the session
among the records a review from the checkout reads through it: the
checkout's own record and its ancestor checkouts' records. A prefix matching
sessions in several of them is refused, and a session several of them hold
is read from the record a review reads it from (one annotating it, then the
one whose log of it is longest, then the smallest id), so a revision builds
on the annotation the review shows. The annotator's brief comes
from one snapshot of that record: the session's transcripts, main first,
each listing its turns in source order with the edits and commands recorded
under them; its delegation observations and derivations; its current
annotation; and the standing corrections naming it, from the review store of
the checkout's repository. A turn id sighted more than once appears once per
retained sighting. No order across transcripts is given, since a worker's
ordinals start at zero.
The brief's record, revision, and the session's observation count and last
observation time are copied into the document as `prepared`, which pins what
the annotator saw. Coverage counts from that pin, never from import time.
The records are reconciled before the session is resolved, so a refused
prefix may follow a sync, which is idempotent and what `review` does before
it can fail.

The `supersigil-annotate` skill in `.claude/skills/` is the annotator. A
person runs it on demand in Claude Code: the host model reads the brief in
parts, writes the document, and imports it. A review names each candidate
session without an annotation and prints the skill command with the
session's whole id, since the skill resolves a prefix among every session
of the checkout's records, and with the review's store options. When the
checkout the review found the session's record through is not the one the
review ran in, the command names it as `--checkout`. That checkout is a
candidate worktree that is or lies in one of the record's associations, so
the record is the checkout's own or an ancestor checkout's, among which
`prepare` finds the session and chooses as the review did. The reviewed
worktree comes first, so the skill
reads the review's own repository and review store. Without one, it is the
record's most specific association lying in a candidate worktree, the
record of a checkout nested in the one reviewed, else the record's first
association. A repository nested in another is thus read through its own
checkout and review store even when only the outer one's record holds the
session.

`annotate import` reads a document of at most 16 MiB and writes to the
record its `prepared.record` names, which must be the checkout's own record
or an ancestor checkout's; a document naming any other, or a record that
does not exist, is refused. It checks the document against one snapshot of
that record, and refuses it whole on any problem, listing up to 100 by JSON
path. Unknown fields are refused. A document not
of the document's shape, such as one with an unknown field or a wrong type,
is refused at the first structural problem, with its path, so structural
problems are reported one at a time. `based_on` must
be the current revision, so of two imports built from one brief the second
is refused as stale. `prepared` must name this record, a revision no later
than the current one, and no more observations than the session has. The
document is checked against the first that many observations, those its brief
was prepared from, so it cites and quotes nothing observed after the brief
however the record has grown since, and `observed.last` must be the latest
time among them, which the review reports it as annotated through. Ids are
1 to 64 characters of `A-Z`, `a-z`, `0-9`, `_` and `-`, unique within their
list. Cites, an attempted alternative's edits, and a question's `about` name
edits or commands of the session. An explicit statement needs a source: a
turn of the session and a quote of 1 to 2,000 characters found verbatim in
one retained sighting of that turn whose role fits the speaker. `human`
needs a human sighting, and `agent` an agent or delegation sighting. A
report sighting is harness-relayed text, not known to be the agent's, so it
backs no explicit statement; an annotator's inference may cite it. A source
is checked whether or not its line is explicit. Titles hold at most 200
characters and texts 2,000. A document holds at most 10,000 decisions,
10,000 questions and 10,000 ids in `corrections_seen`, and 1,000 entries in
any list within a decision or in a question's `about`.

Revisions count from 1 per session and record, each stored as
`interpretations/<n>.a<revision>.json` with its import time beside the
document unchanged. The highest listed is current. Bases and corrections pin
revisions, so none is ever superseded. `annotate list` shows each session of
the checkout's own record with its current revision and the standing
corrections it has not seen.

A review reads each candidate session's current annotation from one record.
When several involved records hold the session, one with an annotation beats
one without, then the one whose pinned logs of the session are longest, then
the smallest id. The choice is per session, while capture limitations are
chosen per transcript. Notes say which record was used when several annotate
the session, and how many observations were recorded since the annotation's
pin.

Decisions reach lines only through the edits they cite. A decision is in
view when an edit it cites contributes to a line of the change. It explains
each span such an edit contributes to and says how: attributed,
whitespace-only, or content match, the last with whether its search
completed. A content match is never shown as attribution, since no chain
explains the line and several decisions can match one span. A decision
citing nothing in the change is not shown. An attempted alternative is
marked restored when the derivations show each of its edits restored.
Decision names per edit live in a separate `edit_decisions` map, so
`EditInfo` is unchanged.

A decision is named by its id, qualified as `<session short>:<id>` when
another annotated session of the review has a decision with that id. The
short form is the shortest prefix of at least 8 characters unique among
those sessions of the session id as a terminal shows it, control characters
escaped, so a name is typed back as shown and two ids shown alike shorten
alike instead of one standing for the other. Quotes, titles, and texts
print through the escaping builder, cut to one line of 200 characters; the
JSON holds them whole.

An open question is a gate item when its `about` is empty or names an edit
contributing to the change. It is named `question:<id>`, qualified by
session only when another question item of the gate has the same id. Its
fingerprint hashes the text and the sorted ids apart, so rewording reopens
it naming `text` and a changed `about` reopens it naming `about`. It leaves
the gate when the current revision drops it.

The basis pins each candidate session's chosen record and revision
(`annotations`) and each decision in view (`decisions`), so a re-annotation
makes a new basis for the same content, related as same-content. No
interpretation enters an evidence fingerprint: explanation digests and
existing gate fingerprints hash the same evidence as before. A re-annotation
therefore reopens no item, and a completion carries when the items match. A
new open question is a new item and blocks carry-over on its own.

When the annotation a review reads differs from the one the most recent
related completion read, the status says so after the code and evidence
lines: `Interpretation changed since completion on <date>: session <short>
revision 1 → 2`, `newly annotated`, or `annotation removed`. Revisions are
numbered per record, so the same number in another record reads `revision
1 → 1 in another record`. The line blocks nothing.

`judge correct <decision>` records a correction: `--wrong` or
`--not-together` with `--reason`, or `--constraint` with the constraint. It
takes the decision's name as the review printed it and resolves it from the
basis alone, since `judge` reads no records; an unknown or ambiguous name is
refused in one line listing the names. The correction references the
decision by record, session, revision, and id. It closes no item and never
keeps a review from completing. Like every `judge` write, it refuses inside
an agent session; agents may run all three `annotate` commands. `judge show`
lists a review's corrections by decision name with their judgment ids.

A review shows the standing corrections naming a candidate session, in any
record and revision, and those recorded on related reviews. A correction
anchors directly to the decision its full reference names when the review
shows exactly that revision from that record. When the review shows another
revision, or one from another record, the correction anchors to each
decision citing an edit the corrected decision cited, as read from the named
record, and says which revision it addressed. A correction whose record the
review does not involve, whose revision cannot be read, or whose exact
revision is shown without the decision in view anchors to nothing. It is
listed under `Corrections with no decision in view:`, never dropped.

On a decision, corrections come before the rationale, oldest first.
`--wrong` and `--not-together` corrections mark it `[disputed]`. A
constraint is also the first rationale line, as the person's statement with
no transcript source. Each correction says whether the shown revision lists
it in `corrections_seen`.
The next brief gives the annotator each standing correction of the session,
with the decision it addressed and the current decisions it anchors to. A
re-annotation clears no mark. Only `judge undo --judgment`, on the review the
correction was recorded on, withdraws one.

`why` takes the store options, `--review-dir` included. It reads the
annotations of every candidate session and the corrections naming them, and
shows the decisions behind its line, saying when the line is only a content
match. To choose records as `review` does, it hashes every pinned log.

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

Judgments and retention refs stay local; git does not push
`refs/supersigil/` by default. Moving a repository starts a new review store;
the old store keeps its judgments. Anyone can unset the agent markers, so
the check prevents accidents only.

Git can prune an unjudged review's captured tree after 14 days because it
has no retention ref. Judging then requires a new review. If a person deletes
a judged review's ref and git prunes its tree, later reviews of changed code
report changed paths as unavailable. Changed evidence excludes conversation
turns and commands whose change reports list no reviewed path, including
reads; an annotation citing them changes only the interpretation.
Unreconciled records or checkouts require new judgments on each review, as
do uncaptured paths whose on-disk state is unreadable or a directory, such
as a submodule.

A reader holding a review manifest for more than a day may find a removed
basis file gone. A reader holding a record manifest more than 14 days after
a later revision replaced a document may find that document gone.

Supersigil detects ordinary concurrent writes through file length, mode,
modification time, and status change time. Deliberate races that restore
timestamps within one kernel clock tick are out of scope; a process of the
same user can write the review store directly. Git capture and Supersigil's
untracked and uncaptured file reads take no locks. Concurrent writes can
produce a combination of file states that never existed together, and
writes through shared memory mappings need not update timestamps. Review a
quiet working tree or a commit.

Supersigil expects to be the only writer of records and the review store.
Manual changes, such as planted links or FIFOs and edited manifests or logs,
can forge any result. Supersigil handles them only to prevent cleanup from
deleting data a current reference needs. Other manual changes may make a
command fail or wait. Working-tree, git, transcript, and sync-written record
changes remain in scope.

Records keep agent, delegation, and report text whole, and the brief hands
it to the annotator through no model-access filter; the host can read the
raw transcript regardless. Thinking is never read. Import checks structure
and that each quote occurs in a fitting sighting of the cited turn. It never
checks that a quote explains the cited edits or that a decision is real, so
a faithful-looking annotation can be wrong. An agent
can import any document that validates; only people correct one. Records
written before agent text was kept whole use an older format and are
refused; once one is moved away, the next sync rebuilds it from the
transcripts still on disk. A session held by two records can be annotated
in each, and a review uses one and says which. A long session makes a large
brief, which the skill reads in parts. The skill runs only when a person
starts it, ships only in this repository's `.claude/skills/`, and needs
`supersigil` on `PATH`.

The intended product adds a terminal UI with final-change and
development-history views, which is not shipped. Until then, a decision
citing nothing in the reviewed change appears in no output.

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
- [Annotation validation](../crates/supersigil-review/src/annotating.rs)
  states every rule an imported document meets.
- [Decisions](../crates/supersigil-review/src/decisions.rs) and
  [their corrections](../crates/supersigil-review/src/decisions/corrections.rs)
  define record choice, names, the decisions in view, question items, and
  correction anchoring.
- [The `annotate` command](../crates/supersigil-cli/src/commands/annotate.rs)
  prepares the brief, imports documents, and lists annotations; the
  [annotate skill](../.claude/skills/supersigil-annotate/SKILL.md) writes them.
- [CLI integration tests](../crates/supersigil-cli/tests/cmd_review.rs) and
  [reconciliation tests](../crates/supersigil-cli/tests/reconcile.rs) exercise
  capture, provenance, uncertainty, and record discovery through user workflows.
- [Review scenarios](../crates/supersigil-cli/tests/scenarios.rs) walk
  reviewing, judging, completing, and returning to a review end to end, and
  annotating, correcting, and re-annotating a session.
