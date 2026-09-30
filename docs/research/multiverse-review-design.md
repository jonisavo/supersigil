# Supersigil: review-first pivot

*Design document. 2026-09-28, revised twice the same day after external
review.*

Status: approved by the author on 2026-09-28, including section 5 (model and
mechanics, rewritten after the second review) and the retention ref in
section 8. Section 6 records how each review point was handled. Section 7
records the author's decisions on points the first review reopened. Section 8
describes the vertical slice that starts implementation.

Amended on 2026-09-29 by the plan 2 design
(`docs/research/2026-09-29-review-slice-2-design.md`): a fifth library
crate for git (section 4), measured facts about Claude Code transcripts
(section 4), how the working-tree target is built and when the retention ref
is written (section 5, snapshots), the review basis pinning several records
(section 5, data model), and the attribution claims and relations (section 5,
attribution). The plan 2 design holds the detail and the reasoning.

Guiding principle, adopted from the first review:

> Supersigil should make the development history easier to inspect without
> making it look more certain, or more coherent, than the evidence allows.

## 1. Context

### What Supersigil is today

Supersigil (v0.14.0) is a Rust CLI, LSP server, and pair of editor extensions
for spec-driven development. Markdown spec files with typed XML components form
a graph of criteria, evidence, and test mappings that the tool verifies in CI
and in the editor. It has one user, its author. Rough sizes:

| Component | Lines |
|---|---|
| LSP server (diagnostics, navigation, completion, code lens, code actions, rename, graph explorer runtime) | 14k |
| Verification crate (criteria, evidence, coverage, `affected`, explorer runtime, git helpers on git2) | 14k |
| Core model and parser | 14k |
| CLI | 9k |
| VS Code extension (TypeScript) and IntelliJ plugin (Kotlin) | 9k |
| Shared graph explorer web app, used by both editors and the website | 8k |

The spec verification machinery is the part being de-emphasized. The editor
work stays valuable but is no longer the first delivery surface (decision 7).

### Why the pivot

Specs and separate design documents rot. Human-authored documentation should be
the priority, and the tool should never become a second system of record for
prose, with one intentional exception: comments and corrections a person
writes inside Supersigil are primary records. The author wants Supersigil to
augment human craft with what LLMs are good at: exploring at breadth,
visualizing a system, showing what led to its current shape, and surfacing
what could be simpler.

### Influences and what we verified about them

- [Understand Anything](https://github.com/Egonex-AI/Understand-Anything)
  (MIT) turns a codebase into an explorable knowledge graph using tree-sitter
  for structure and an LLM for meaning. It runs inside the host agent as slash
  commands and subagents, and its README documents plugin discovery in Cursor
  and in VS Code with Copilot. It writes a JSON graph and opens a separate web
  dashboard. It has no native review or navigation experience inside an
  editor; the useful distinction for Supersigil is that, not "no IDE".
- [Delta](https://delta.dev/) from Zed Industries keeps agent conversations and
  code together in threads on DeltaDB, a CRDT-based stream of fine-grained
  deltas where references anchor to a delta rather than a line number. Commits
  stay in git; the thread history lives in DeltaDB, which sits beside the repo
  and replicates between clients. As of September 2026 no supported
  integration surface for reading DeltaDB has been established in the public
  material. The [roadmap](https://delta.dev/roadmap) lists a Claude Code
  plugin as in progress ("Track Claude Code conversations and code changes in
  DeltaDB, then review them in Delta") and a graph view, conversation
  branching, and repository-level context as up next. Those are commitments,
  not shipped capabilities, but they overlap this design directly. Sources:
  [Delta and Git](https://delta.dev/docs/concepts/delta-and-git),
  [Software Is Made Between Commits](https://zed.dev/blog/introducing-deltadb),
  [Delta public beta](https://zed.dev/blog/delta-public-beta).
- mvs, an earlier prototype by the author (Bun CLI, winter 2025 to 2026). A
  "universe" is a git branch plus worktree exploring one hypothesis, Prime is
  main, convergence merges or replaces. Decision records, domain documents, and
  workstreams sit beside it as Markdown with front matter. It stopped in
  February 2026. This design keeps the multiverse as a way of seeing history,
  derived from data, not as something anyone manages by hand.
- The author's April 2026 "specs in source" research already reframed the
  graph as the artifact with pluggable inputs. That carries over.

### Where Supersigil should differ from Delta

Delta records operations with stable identities at the moment they happen.
Supersigil reconstructs provenance afterwards from a weaker capture source and
must never imply equivalent certainty. Its distinction is evidence-conscious
review that stays useful with incomplete history, runs where the developer
already is (a terminal, including an IDE's integrated terminal), keeps the
record local and in an open format, and treats a person's corrections as
first-class. Provider-native IDs are preserved in the abstraction so a
supported Delta export could become a source later, at essentially no cost
now.

## 2. Decisions made

1. **Primary use case is review.** Reviewing a change, yours or an agent's,
   with the recorded rationale attached as evidence. Archaeology ("why is this
   here?") reuses the same record later. Understanding an unfamiliar codebase
   is a later extension that needs structural analysis the record does not
   provide, especially for code that predates it.
2. **Rationale comes from agent sessions first.** Claude Code transcripts
   first, Codex second. An annotator agent marks decisions in a transcript.
   Human corrections arrive in the first arc, not later: "this explanation is
   wrong", "these edits do not belong together", "the actual constraint is
   this". They are durable, survive re-annotation, and have defined effects
   (section 5).
3. **The host harness runs every LLM step.** Supersigil stays deterministic and
   holds no API key. All LLM work is a skill or subagent in Claude Code that
   writes results back through the CLI. This says nothing about where the
   host sends its model calls; the privacy policy in section 5 covers that.
   An Agent Client Protocol bridge is a planned upgrade, not part of the first
   arc.
4. **Local-only record now; publication is an export, not a copy.** Raw
   transcripts stay where Claude Code puts them. The record lives under the
   user's home directory with portable identities and a versioned schema. When
   the record is shared later, a sanitized, self-contained bundle is exported
   after a preview, with withheld evidence marked. Anchors carry git identities
   as evidence of where an observation was made but never depend on them.
5. **Review first, thin core.** Build the pipeline from transcript to edits to
   code, the annotator skill, and the review experience. The map view and the
   code knowledge graph are later arcs.
6. **Full purge first.** Revised on 2026-09-28 from "the spec crates stay
   untouched until the first arc ships". The spec-driven implementation, its
   LSP, editor extensions, npm packages, docs site, project specs, and spec
   workflow skills are removed from the working tree and archived under the
   `archive/spec-driven` tag; the 0.14.0 releases stay published. The CLI
   crate is reduced to a shell. The workspace lint policy, which follows
   Microsoft's Pragmatic Rust Guidelines, is kept verbatim. Plan 0 in
   `docs/research/2026-09-28-review-slice-0-purge.md` carries it out.
7. **A terminal UI is the first surface.** A TUI runs everywhere, including
   every IDE's integrated terminal, and needs no extension packaging. It is a
   delivery shortcut, not a replacement for the later editor experience. The
   LSP requests, code lens, hover, and the shared explorer views come in a
   later arc, driven by the same review model, which is why the review model
   crate stays independent of the TUI.
8. **The multiverse is a view, not the truth model.** Conversation history,
   observed filesystem history, and git history are three linked records, not
   one tree. Interpretations sit in a separate layer with provenance.

## 3. Product concept

### Three histories, linked

- **Conversation history.** Which turn descended from which. Claude Code's
  transcript already has parent pointers, and `/rewind`, `/branch`, and
  `--fork-session` fork this history without touching files.
- **Observed filesystem history.** Which edit transformed which file state
  into which. Each edit keeps both sides and its own identity. When one edit's
  starting state does not match the previous known state of that file, the
  record shows the discontinuity instead of bridging it.
- **Git history.** Which snapshots became commits and how they relate.

A rewound conversation with kept code, two worktrees on unrelated tasks, and a
session spanning three commits are all representable without inventing a
filesystem branch that never existed. An untried alternative is a proposition
in the interpretation layer, drawn as a dotted stub, never a code state.

### The review unit

A review is a selected change between a base snapshot and a target snapshot,
within a stated scope. Both snapshots are complete git trees. For the working
tree, Supersigil builds the complete target tree from a temporary copy of
the checkout's index updated with the saved on-disk state, including
deletions, so two different deletions never share an identity. The scope states what the review
includes: tracked changes always; untracked files, path filters, binary and
unsupported files, and mode-only changes according to explicit options with
stated defaults. Sessions are contributors to a review. Selecting a session
filters the review; it does not define it. Unsaved editor buffers are out of
scope for the first arc, and the review is marked outdated when the target
changes.

### Two synchronized perspectives

- **Final change.** Everything that differs between base and target within
  the scope. A plain file-oriented diff is always available. Grouping by
  decision is a toggle. Every changed span shows what evidence connects it to
  recorded edits, or says that nothing does. Unattributed spans are summarized
  prominently and stay in the diff; they may be the most important material
  in the review.
- **Development history.** The rail: prompts, decisions, test runs, restores,
  and the stubs for alternatives, in time order, as a vertical branch graph.
  An attempt that was built and restored appears as a one-line summary in
  place, with its historical patch expandable. Its existence is visible
  without a click; reading it is never mandatory. Its status is conveyed by a
  marker and label, not by dimming alone.

Neither perspective replaces the other. Walk mode steps through either one
item at a time.

### Recorded rationale, not "the reason"

The interface distinguishes a human instruction, an agent's stated
explanation, and the annotator's retrospective inference, and shows the
source span for each. "No rationale recorded" and "no alternatives recorded"
are valid states. "Content was restored" is an observation; "this approach was
rejected because it failed" is an interpretation and is labeled as one, with
its source or an inference mark. A causal arrow from a restore to a later
decision is drawn only when the interpretation layer records it.

### Consequences, named for what they are

Each decision group ends with lines named for the evidence behind them:

| Evidence | Shown as |
|---|---|
| Textual identifier matches in the target snapshot, bounded scope | Other occurrences of this identifier |
| Captured test output, with a freshness statement (section 5) | Tests observed running, or no run observed |
| Documentation path patterns | Documentation files changed |
| Annotator proposals | Proposed implication, labeled as such |

Cross-session retrieval of potentially related earlier discussion is a later
arc.

### Closing gate

The review ends with a gate that gathers what has not been dealt with:
unacknowledged consequences, unattributed spans, lineage discontinuities,
unresolved anchors, test commands with no observed run against the reviewed
state, and the annotator's open questions. Marking the review complete in
Supersigil requires each gate item to be acknowledged or dismissed with a
short reason, and both are judgments bound to the review basis. Gate items
have stable identities and a defined granularity (section 5), so rewording an
unchanged item does not create a new obligation and replacing its evidence
does not silently keep an old acknowledgment. Reading an item, agreeing with
its explanation, and marking the review complete remain separate actions, and
accepting the change itself is done with git, outside Supersigil. The first
external review argued for making acknowledgment optional; the author chose
to keep the gate.

### Human contributions

Acknowledgments, dismissals, comments, corrections, and completions are
judgments bound to a review basis: the exact base and target trees, the
scope, and the evidence revisions the review was built from. Later, the
current view reports separately whether the code changed, whether the
annotation changed, or whether new relevant evidence arrived, and the original
judgment is never edited. A correction stays attached to the interpretation
it addressed, is shown ahead of that interpretation, marks it disputed, and is
handed to the next annotation pass. Comments attach to a stable historical
anchor so they survive a re-annotation that splits or regroups decisions.

### Vocabulary

Session, decision, rationale, alternative, attempt, restore. "Decision" means
an interpreted choice only, never an umbrella for prompts, test runs, and
restores. "Multiverse" names the view. "Prime" is optional vocabulary; the
interface names the actual base branch or commit.

### Acceptance criterion for the first arc

Open a selected diff in the terminal. Inspect every changed span within the
stated scope. Recover supported context where the evidence allows it, see
uncertainty where it does not, and preserve the reviewer's judgment against
that exact code and evidence state.

## 4. Architecture (agreed)

Five new library crates, one TUI crate, and CLI commands. Editors and the
LSP come in a later arc. All library crates except `supersigil-git` are
deterministic and pure over their inputs; `supersigil-git` is the one that
runs git, so the others stay testable without repositories.

- **supersigil-session.** Transcript sources behind one trait, Claude Code
  first. Discovery uses the `transcript_path` and `cwd` that Claude Code hooks
  provide as the primary route, with a scan of the encoded project directories
  as the fallback for sessions recorded before the hook existed. Parses JSONL
  tolerantly: unknown record types are counted and reported where they could
  affect attribution; an incomplete final line of a live file is retried, not
  treated as corruption. A `user` record is not assumed to be human intent;
  tool results, meta records, and compaction summaries are classified apart.
  Emits observations: turns with parent pointers, prompts, replies, edits with
  both sides and the structured patch, commands with output, exit status, and
  start and end times, and subagent sidechains.
- **supersigil-record.** The store, with four layers that have different
  semantics: observations are immutable events; derivations are rebuildable
  findings tagged with the inputs and algorithm version that produced them;
  interpretations are superseded, never overwritten; contributions are durable
  and never edited. The record has its own identity and a manifest that pins
  exactly which immutable files and committed log lengths make up a revision,
  so a reader never mixes revisions.
- **supersigil-anchor.** Two operations kept apart. Resolution asks where a
  historical fragment corresponds in a chosen target snapshot and answers
  exact, candidate, ambiguous, deleted, or unresolved. Attribution asks what
  evidence connects a span of the reviewed change to recorded edits and
  decisions: exact patch correspondence within the observed chain when the
  chain composes, labeled content matching when it does not, with capture
  completeness reported separately. A span can have several contributors, in
  named relations. Symbol anchors through tree-sitter are a later arc.
- **supersigil-git.** Everything that talks to git, through the git CLI so
  the working-tree target is exactly the tree `git add` would write under
  every filter, index, and hook configuration: snapshots of a base and a
  target, changed paths, blob bytes and their worktree-form conversion,
  worktrees, and the per-worktree reflogs that say which worktree originated
  a commit. It never writes the real index, the worktree, or a ref (the
  retention ref is written by the judgment path, section 5).
- **supersigil-review.** Pure functions that build the review model from a
  review basis, the record, and anchors: both perspectives, decision groups,
  attempts, consequences, and the closing gate with stable item identities.
  JSON serializable for agents and for later surfaces. Independent of the
  TUI.
- **supersigil-tui.** A ratatui and crossterm interface launched by
  `supersigil review` when stdout is a terminal. A rail pane, a main pane with
  the two perspectives, and an inspector for the selected item. Diff text is
  syntax highlighted from the first arc through syntect, the library behind
  bat and the delta pager, with its bundled syntax set; tree-sitter can take
  over highlighting later when it arrives for symbol anchors. Control and
  escape sequences in rendered text are shown as visible escapes rather than
  deleted, so the reviewer sees what the bytes are; the original evidence is
  untouched.

**The split between machine and model.** Supersigil computes observations,
derivations, and the skeleton: prompts, edit clusters, test runs and outcomes,
restores detected by content, forks in the conversation tree, and
discontinuities. The annotator, a Claude Code skill, adds decisions with
rationale, alternatives, and proposed implications, each with source spans,
speaker, an explicit-or-inferred mark, and the annotation revision. The
importer validates structure, every referenced ID, and the rule that an
explicit human or agent statement carries a source span. Semantic faithfulness
is tested by evaluations and human inspection, not by the importer.

**Capture is reconciled, not assumed.** A Claude Code stop hook runs a cheap
incremental sync, but Stop does not fire on interrupts or API errors and the
transcript may lag when it does, so opening a review always reconciles first.
Sync is incremental, non-blocking, and recoverable. Because Claude Code keeps
only the 100 most recent checkpoints per session and sweeps them after about
30 days, the record retains the patch material it will later show; a
reference to a backup file is never the only copy.

**Surfaces.** CLI: `session`, `review`, `why`, `annotate`, and `record`
commands, all with JSON output. TUI: `supersigil review`. Skills: annotate and
review. Hook: stop-hook snippet installed by `supersigil init`.

### Facts about Claude Code transcripts and hooks, verified 2026-09-28

- Transcripts live at `~/.claude/projects/<cwd with slashes replaced by dashes>/<session id>.jsonl`.
  Each worktree gets its own directory. Hooks receive `session_id`,
  `transcript_path`, `cwd`, and `hook_event_name` on stdin, plus `agent_id`
  and `agent_type` inside subagents.
- Record types seen: `user`, `assistant`, `system`, `attachment`,
  `file-history-snapshot`, `file-history-delta`, and several UI-state types.
- Every `user` and `assistant` record carries `uuid`, `parentUuid`,
  `sessionId`, `cwd`, `gitBranch`, `timestamp`, `version`, `isSidechain`,
  `isMeta`, and `agentId`. `user` records also carry tool results.
- An Edit tool result carries `filePath`, `oldString`, `newString`,
  `originalFile`, `structuredPatch` (hunks with `oldStart`, `oldLines`,
  `newStart`, `newLines`, `lines`), `replaceAll`, and `userModified`.
- Claude Code backs up file versions under
  `~/.claude/file-history/<session id>/<hash>@v<n>`. Checkpoints do not
  track shell edits, most subagent edits, or external changes.
- These are observations from one machine and one Claude Code version. They
  are the starting corpus, not a contract; the parser degrades visibly when a
  field is absent.

Measured on 2026-09-29 over every transcript on the author's machine (1,058
Edit and 860 Write calls, Claude Code 2.1.258 to 2.1.284):

- `originalFile` is present only for small files (at most 8,996 characters
  observed) and null for larger ones, so the before- and after-states of
  most real edits carry no content hash.
- `structuredPatch` is a display rendering, not bytes: tabs are expanded.
  `oldString`, `newString`, and Write `content` are byte-faithful; replacing
  the located `newString` with `oldString` reproduced `originalFile` for all
  151 edits that had one. Attribution and every rendering of a historical
  patch use the replacement texts, never the hunks; attribution uses a hunk
  only as a consistency check where no recorded hash decides, under the
  display rule measured over the same corpus (every tab shown as two spaces,
  274 of 274 hunks), and ignores a hunk that contradicts the rule (one holding
  a literal tab). A rule changed in a way that check cannot see could reject
  the true reading of a hashless edit and hide an alternative; that is the
  stated cost of using the rule.
- MultiEdit does not occur.

## 5. Model and mechanics (draft, awaiting the author's review)

### Layers

| Layer | Rule | Examples |
|---|---|---|
| Observations | Immutable events, append-only | turns, edits, commands, session start and end events |
| Derivations | Rebuildable; each carries the observation revision and algorithm version it was computed from, and is superseded by recomputation | restores, discontinuities, git associations, path lineage, skeleton, carry-forward associations |
| Interpretations | Revisioned; a new revision supersedes, never overwrites | annotations |
| Contributions | Durable, never edited; corrections are written by people only | judgments |
| Indexes | Rebuildable | lookups |

A session's current summary (started, ended, branch) is a read model built
from its start and end events, not a mutable record.

### Data model

```
# Observations
SessionStart { session, source, source_ids, record_id, checkout, branch, time }
SessionEnd   { session, time, reason }
Turn      { id, session, parent?, role: Human | Agent | Tool | Meta | Summary,
            time, sidechain, agent_id?, excerpt?, event_ids }
Edit      { id, turn, path, before: FileState, after: FileState,
            patch: Material<[Hunk]>, old_text: Material<Text>, new_text: Material<Text>,
            replace_all, checkout, time, source_ordinal }
Command   { id, turn, cmd, exit?, stdout_tail: Material<Text>, stderr_tail: Material<Text>,
            kind: TestRun | Build | Git | Other, started, ended?, checkout }
FileState = Absent | Present { content: Known(ContentId) | Unknown }
Material<T> = Retained(T) | Withheld { policy } | Unavailable { reason }

# Derivations (tagged with { observation_revision, algorithm_version })
Restore       { edit, restores: [EditId] }             # after == an earlier before; may list candidates
Discontinuity { path, checkout, after: EditId, before: EditId }
PathLineage   { from_path, to_path, basis: ObservedCommand | GitRename, certainty }
GitAssoc      { edit, commit?, blob_before?, blob_after?, certainty: Exact | Likely | Unknown }
CarryForward  { judgment, applies_to: ReviewBasisId, basis: EvidenceFingerprint }

# Interpretations
Annotation { revision, session, produced_by, inputs: { observation_revision, corrections_seen },
             time, decisions, open_questions }
Decision   { id, title, rationale: [Rationale], cites: [EventId],
             alternatives: [Alternative], implications: [Implication] }
Rationale  { text, source_span?, speaker: Human | Agent | Annotator, explicit: bool }
             # explicit == true requires source_span; enforced on import
Alternative{ title, status: Proposed | Attempted { edits, restore? }, reason?: Rationale }
Implication{ text, source_span?, explicit: bool }
Motivates  { from: RestoreId, to: DecisionId, basis: Rationale }

# Contributions
ReviewBasis { id, base: TreeId, target: TreeId, scope: Scope,
              record_revisions: [(RecordId, revision)],
              annotation_revisions: [(session, revision)],
              derivation_version }
              # one review can involve several records: a sibling worktree
              # or a subdirectory checkout has its own
Judgment    { id, kind: Ack | Dismiss | Comment | Correction | Complete,
              basis: ReviewBasisId, decision?, gate_item?: GateItemId,
              anchor?, who, time, text? ,
              correction_kind?: ExplanationWrong | EditsDoNotBelongTogether | ActualConstraint }

# Review model (computed, not stored)
GateItem { id = hash(origin, subject), origin, subject, evidence_fingerprint, revision }
```

`ContentId` is a hash of file content; absence is a distinct state, never an
invented hash, and an unknown preimage is never shown as a known empty one.
`Material` distinguishes retained evidence from evidence withheld by policy or
unavailable from the source. Renames are not edits; they are derived lineage
with a stated basis. `source_ordinal` preserves the transcript's own order so
timestamps are used for display, never to break ties in lineage.

### Snapshots and scope

The base is a commit tree, or the empty tree when there is no commit. The
working-tree target is a complete tree built from a temporary copy of the
checkout's index, updated with the saved on-disk state as `git add -u` would
(and with explicitly listed untracked paths), honoring the scope's inclusion
options, and written to the repository's object database; paths whose
on-disk state that procedure does not capture are listed. When the first
judgment binds to a basis, a ref under `refs/supersigil/reviews/<basis id>`
is created pointing at a commit wrapping that tree, so the reviewed bytes
survive garbage collection and a completed review can be reopened later; the
ref is local and never pushed. Reviews that nobody judged write no ref, since
every sync mints a new basis. If
the capture policy forbids retaining the bytes, the basis records that the
target is not reproducible, and reopening shows the judgments without the
diff. Scope defaults for the first arc: tracked changes included, untracked
files excluded unless listed, binary files listed but not diffed, mode-only
changes listed.

### Review basis and change detection

Every judgment references a review basis. When a review is opened, the current
basis is compared with any completed one for the same base and target:

| Difference | Reported as |
|---|---|
| Target tree differs | Code changed since review |
| Annotation revisions differ for a contributing session | Interpretation changed since review |
| Record revision differs and new observations touch the reviewed paths or contributing sessions | New evidence since review |
| Only unrelated observations were appended | No change |

The original completion is kept and shown with its date. Acknowledgments are
carried forward only through a derived association whose evidence fingerprint
matches; a fingerprint mismatch leaves the item open again with the old
acknowledgment visible in history.

### Gate items

A gate item's identity is the hash of its origin and its subject, so a
rewording does not create a new item. Its evidence fingerprint covers the
material behind it, so replaced evidence reopens it. Granularity:

| Origin | One item per |
|---|---|
| Consequence or proposed implication | decision |
| Unattributed span | file |
| Discontinuity | path |
| Unresolved anchor | decision |
| No run observed | test command family (for example `cargo nextest`) per review |
| Annotator open question | question |

An item disappears when its condition no longer holds.

### Test freshness

A test run's freshness is stated against the reviewed target using only
observed events: "no later changes observed in the tracked scope" when no edit
or discontinuity on paths in the review scope was observed after the run
ended; "changes observed after this run" otherwise; "freshness unknown" when
the run lacks an end time or a discontinuity overlaps it. A run before the
last relevant edit stays visible but is never presented as verification of
the final code.

### Attribution

Two claims are kept apart. Transformation exactness: the retained
replacements, each validated by executing it forward with the recording
tool's semantics, compose against their recorded inputs and produce the
target bytes; this is checked, and when it holds, spans are tracked through
the composition. Capture completeness: the observations account for
everything that happened between the states; this is never claimed. Because
most real edits carry no content hash (section 4), a chain is classed by
what verifies it: *exact from the base* when replaying it from the base
reproduces the target, *exact from its start* when every step's recorded
hashes verify but its start is not the base (a gap), and *consistent* when
neither holds, reported as "consistent with the recorded edits; not
verified". A discontinuity is a detected gap in the available observations,
not a promise that every unobserved transition is detectable.

Contributors to a span are recorded in named relations: introduced the
retained content, replaced base content, changed only whitespace, or
participated earlier in the region's history. Grouping the final diff by
decision uses introduced and replaced, so a formatter pass neither erases
the decision beneath it nor claims authorship. A whitespace-only change is
an observed property, not a claim that presentation alone changed (whitespace
can matter inside strings and in indentation-sensitive code), so it stays
visible to decision grouping or the gate and is never dropped as formatting.
When several readings of different recorded events fit the evidence, the
span is reported ambiguous with the competing chains, and time does not
decide. A reading that differs from one reaching the base only in something
the record does not hold (where a replacement happened, the order of calls
one message issued) or in how far back the recorded evidence reaches (the
base-reaching reading may continue into sessions the other never reached),
holds no recorded edit that one lacks, and needs an
unrecorded change to explain its start is listed as an alternative that
assumes an unrecorded change, not combined: that is the claim every exact
chain makes, since capture completeness is never claimed. A reading holding
recorded evidence the base-reaching one lacks is never set aside.

Attribution across checkouts uses the record's associations. A commit target
with no originating-checkout association is attributed only through git
associations of exact certainty; matching contents never infer a checkout.

### Record layout and revision protocol

```
$XDG_DATA_HOME/supersigil/records/<record_id>/
  manifest.json                      identity, schema version, revision, associations,
                                     and for every file below: name plus committed length
  observations/<n>.jsonl                     append-only
  derivations/<observation_revision>.<algorithm_version>.json   immutable per generation
  interpretations/<n>/<revision>.json        immutable per revision
  contributions/judgments.jsonl              append-only
  index/                                     rebuildable, not pinned
```

`<n>` is the number the manifest assigns a session on its first write.
Session ids come from transcripts, so they never become file names.

The manifest pins a revision precisely: for append-only logs, the committed
byte length; for generated documents, the immutable file name. Readers load
one manifest and read only what it pins, so a reader never mixes revisions
even while a writer appends. Writers take a lock, read the manifest revision,
append to logs or write new immutable files, fsync, then publish a new
manifest by rename with the expected prior revision checked. A crash leaves
at most a tail beyond a pinned length or an unreferenced file, both ignored
and cleaned at next open. Unreferenced files older than a grace period are
removed.

### Corrections

A correction is a judgment against a specific interpretation revision. Its
effects are defined:

- It stays attached to the material it addressed and is shown ahead of it,
  which is marked disputed or corrected in every view.
- The next annotation pass receives applicable corrections, subject to the
  model-access policy, and the annotation records which corrections it saw.
  Regenerating the same explanation does not resolve the dispute; the mark
  stays until a person withdraws the correction with a new judgment.
- "Explanation wrong" disputes a rationale. "Actual constraint" adds a
  human rationale with speaker Human and explicit true, shown first.
- "Edits do not belong together" records disagreement with a grouping and
  marks the group disputed. In the first arc it does not regroup, and the
  interface says so.

### Data flow

1. **Sync.** From the stop hook or on demand, and always on review open.
   Discover transcripts, parse each one whole, append the observations that
   lie past its cursor, recompute derivations for changed sessions, publish a
   revision. Idempotent.
2. **Annotate.** The annotate skill runs `supersigil session show <id>
   --format json`, which emits the skeleton, applicable corrections, and turn
   text, every field filtered by the model-access policy. The skill writes an
   annotation and runs `supersigil annotate import`, which validates and
   stores a new revision and recomputes carry-forward associations.
3. **Review.** `supersigil review [--base <ref>] [--target <ref> | --working-tree] [scope options]`
   reconciles, builds or finds the basis, resolves and attributes, builds the
   model, and opens the TUI or prints JSON.
4. **Judge.** Acknowledge, dismiss, comment, correct, or mark complete from
   the TUI or CLI. Appends a judgment bound to the basis. Marking complete is
   refused while any gate item lacks an acknowledgment or dismissal.

### Privacy boundaries

- **Capture.** Which repositories, paths, and payload classes Supersigil may
  retain locally. Applies to prompts, edit contents, command output, and
  annotator text alike. Human authorship is not a reason to retain.
- **Model access.** Which fields the annotate skill may receive, evaluated per
  field. Paths, command arguments, titles, and identifiers are structural but
  not harmless; the skeleton is filtered like everything else.
- **Publication.** Which material may leave the private record, chosen
  explicitly after a preview with default exclusions. Withheld evidence is
  marked in the bundle so missing context is explicit. Redaction that removes
  bytes an edit needs is recorded as withheld material, not silently applied.

Supersigil controls only its own disclosure. It filters what it hands to the
host; whether the host can read the raw transcript or the repository through
its other tools depends on the host's configuration, and the documentation
says so. Transcripts are untrusted input: commands seen in a session are
never replayed, paths in edits are validated against the checkout boundary,
and rendered text shows suspicious bytes as visible escapes.

### Error handling

- Unknown record types and malformed lines are counted and attached to the
  affected session; the review shows a capture limitation where it could
  explain a missing link.
- A missing backup file falls back to the retained patch material; if that
  is withheld or unavailable, the edit is shown with that state.
- Unresolved and ambiguous are acceptable outcomes, shown as such, never
  dropped or explained away.
- Annotation import rejects unknown IDs, explicit statements without a
  source span, and schema violations, with JSON paths, and refuses partial
  imports.
- A deleted transcript leaves the record intact and marks the transcript
  unavailable.
- A working-tree target that changes during review marks the review
  outdated.

### Testing

- Fixture transcripts, redacted real ones and synthetic ones. Decisive cases:
  conversation-only rewind, pure deletion, file creation, identical text in
  two worktrees, a manual edit inside a chain, an unrecorded excursion that
  returns to the same content, a partially retained attempt, stale test
  results, and re-annotation after acknowledgment.
- Golden snapshots for observations, derivations, and skeletons.
- Property tests for chain composition and resolution under insertions,
  deletions, moves, and formatting-only rewrites.
- Git fixture repositories for attribution across commits, working tree,
  renames, squashes, and the two-deletion snapshot case.
- Store tests: crash recovery at every step of the publish sequence, and
  interleaved readers and writers asserting that no review is ever built from
  a mixture of revisions.
- TUI snapshot tests through ratatui's test backend, including visible
  escaping of control bytes.
- Schema tests for annotation import, including the explicit-requires-span
  rule.
- Skill evaluations in the existing `eval/` harness measuring false
  attribution and unsupported explanation, not only linkage: were
  alternatives actually recorded, is each rationale supported by its cited
  span, was an explicit human constraint preserved, did a correction reach the
  next pass.

### First arc scope

Included: Claude Code ingestion, the four-layer record with pinned revisions,
complete-tree snapshots with a local retention ref, attribution with named
contributor relations and visible discontinuities, the annotate skill and
import with provenance rules, judgments bound to a review basis with change
detection, corrections with defined effects, the TUI with both perspectives
and syntax highlighting, observed test results with freshness statements,
exact restores as attempts, the closing gate with stable items, and
`supersigil why <file>:<line>` as a thin command over attribution.

Deferred: the map view, cross-session retrieval and conflict proposals,
tree-sitter symbol anchors, the Codex source, the Agent Client Protocol
bridge, the publication bundle (identities are portable from day one),
regrouping from corrections, the LSP and editor surfaces, and insight
generation beyond the consequences above.

## 6. Review disposition

### First review

| Review point | Disposition | Where it landed |
|---|---|---|
| Review-first is right; onboarding does not "fall out" | Accepted | Decision 1 |
| Recorded rationale, three speakers, human corrections in arc 1 | Accepted | Decision 2, section 3, data model |
| Soften the closing gate | Declined by the author: the gate stays, with acknowledge-or-dismiss required to mark a review complete | Closing gate, section 3 |
| Multiverse as a view; three histories linked, not collapsed | Accepted | Decision 8, section 3 |
| Review unit is a change, sessions contribute | Accepted | Section 3 |
| Restores are observations, causality is interpretation | Accepted | Restore, Motivates, section 3 |
| Separate resolution from attribution; edit identity; both sides; gaps; multiple contributors; latest-wins only inside lineage | Accepted | supersigil-anchor, section 5 |
| Keep git identities as evidence | Accepted | GitAssoc, decision 4 |
| Resolution relative to a target with explicit outcomes | Accepted | supersigil-anchor |
| Events are not Decision variants; rationale provenance; faithfulness evals | Accepted | Data model, testing |
| Transcript format is observed input; `user` is not always human | Accepted, confirmed by `isMeta` and tool results in real records | supersigil-session |
| Discover via hook `transcript_path` | Accepted as primary; directory scan kept as fallback for pre-hook history | supersigil-session |
| Stop hook is not a capture guarantee; retry incomplete line; retain patch material | Accepted, verified in Claude Code docs | Section 4 |
| JSON fine; layers with different semantics; comments are primary records | Accepted | Record, section 1 |
| Replace root-commit repo id | Accepted | Record identity |
| Atomic revisions for concurrent writers | Accepted, then made precise after the second review | Revision protocol |
| Publication is an export | Accepted; revises decision 4 | Decision 4 |
| Review state bound to snapshot and revisions; changed since review; saved on-disk target | Accepted, then extended to an evidence basis after the second review | ReviewBasis |
| Consequences under narrower names; search the snapshot; test freshness | Accepted | Section 3, test freshness |
| Cross-session as retrieval, conflict left to annotator | Deferred from arc 1 entirely | First arc scope |
| Privacy on all payloads; three boundaries; export preview; untrusted transcripts | Accepted | Privacy boundaries |
| Two synchronized perspectives; plain diff always available; attempts expandable; unattributed prominent | Accepted | Section 3 |
| Do not turn the graph renderer into a diff renderer | Moot: TUI first (decision 7). Transport pattern kept for the later editor arc | Section 4 |
| Narrower acceptance criterion; test false attribution | Accepted | Section 3, testing |
| Delta overlap; soften "cannot be a source"; watch for an export interface | Accepted | Section 1 |
| Understand Anything correction | Accepted | Section 1 |
| Vocabulary | Accepted | Section 3 |

### Second review

| Review point | Disposition | Where it landed |
|---|---|---|
| Working-tree identity built from surviving diff paths conflates different deletions | Accepted; confirmed by construction. Target is now a complete tree built through a temporary index | Snapshots and scope |
| Scope must be explicit: untracked, filters, unsupported types, mode-only | Accepted, with first-arc defaults | Snapshots and scope, review unit |
| Retain reviewed bytes, subject to capture policy | Accepted; local ref under `refs/supersigil/reviews/` pins the target, with an explicit not-reproducible state when policy forbids | Snapshots and scope |
| Completion must bind to an evidence basis, distinguish code changed, interpretation changed, new evidence | Accepted | ReviewBasis, change detection |
| Gate items need stable identity, evidence fingerprint, granularity | Accepted | Gate items |
| Test freshness needs a defined basis | Accepted, with three conservative statements | Test freshness |
| "Exact" claim too strong: transformation exactness versus capture completeness | Accepted, wording and reporting changed | Attribution |
| Time orders display, not lineage; preserve source ordering; ambiguity stays ambiguous | Accepted | `source_ordinal`, attribution |
| A commit target without checkout association must not infer one from content | Accepted | Attribution |
| Define "contributor" | Accepted, four named relations | Attribution |
| Manifest-last publication does not give consistent reads | Accepted; manifest now pins committed lengths and immutable file names | Revision protocol, store tests |
| Gap, Restore, GitAssoc are derived, not observed; Session.ended in an immutable record | Accepted; new derivations layer, session start and end events | Layers |
| Represent absence, unknown content, withheld material, renames | Accepted | FileState, Material, PathLineage |
| Corrections need defined behavior; carry-forward is derived; explicit requires source span | Accepted | Corrections, CarryForward, importer rule |
| Skeleton is not harmless; state the host boundary honestly; visible escaping instead of stripping | Accepted | Privacy boundaries, supersigil-tui |
| Build one small vertical slice before anything else | Accepted | Section 8 |

Nothing in either review was rejected on technical grounds. The reviewer's
proposal to store snapshot material was met with a git-native mechanism, the
retention ref, that the reviewer did not specify; it is flagged for the author
in section 8.

## 7. The author's decisions on reopened points

1. **Closing gate: kept.** Consequences inline plus a closing gate that
   requires acknowledge-or-dismiss before a review is marked complete.
2. **The map: kept as a later arc**, despite Delta's roadmap listing a graph
   view. The rail inside the review carries the multiverse idea in the first
   arc.
3. **TUI: ratatui plus crossterm**, launched by `supersigil review` when
   stdout is a terminal and printing JSON otherwise, with syntax highlighting
   through syntect from the first arc.

## 8. Next: the vertical slice

Implementation starts with one deliberately small end-to-end slice, before
any breadth:

1. One checkout and one synthetic session containing an ordinary edit, a pure
   deletion, a file creation, a restore, an unobserved interruption, and a
   recorded test run.
2. Produce the normalized record with all four layers and a pinned revision.
3. Import one annotation.
4. Open the working-tree review in the TUI, inspect both perspectives,
   correct one explanation, and complete the review through the gate.
5. Then alter the working tree, and separately add relevant evidence without
   altering the code. Verify that the system reports "code changed since
   review" and "new evidence since review" as distinct states while the
   original judgments are preserved unchanged.

Pinning the reviewed target tree with a local ref under
`refs/supersigil/reviews/` was Claude's proposal rather than a reviewed
decision. It writes objects and a ref into the user's repository, which is
what `git stash` does. The author approved it on 2026-09-28.
