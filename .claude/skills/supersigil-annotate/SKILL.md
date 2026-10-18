---
name: supersigil-annotate
description: "Annotate a recorded session's decisions for Supersigil: read its brief, write sourced decisions, import them."
argument-hint: "<session> [--record-dir=…] [--claude-home=…] [--checkout=…] [--review-dir=…]"
disable-model-invocation: true
---

# Annotate a session

You write the annotation of one recorded Claude Code session: the decisions it made, each citing the
edits that carried it out, with every line of rationale labelled by who gave it. `supersigil review`
shows each decision on the lines its edits explain. Import checks citations and quotes against the
record; it cannot check that an explanation is right. A reviewer trusts a line labelled as said, so
label every inference as yours.

`supersigil` must be on `PATH`: run `command -v supersigil` first, and if it prints nothing, tell
the person that `supersigil` is not on `PATH` and stop. Of Supersigil's commands, run only
`supersigil annotate`; corrections are the person's, made with `supersigil judge`.

## 1. Session

The arguments are a session id or a unique prefix of one, then any store options, as
`supersigil review` prints them:

    /supersigil-annotate <session> [--record-dir=…] [--claude-home=…] [--checkout=…] [--review-dir=…]

Every `supersigil` command below ends with those options exactly as given, quoting included;
`<options>` stands for them. Without a session, run
`supersigil annotate list --format terminal <options>`, show the person its table of sessions and
their current annotations, and ask which session to annotate.

## 2. Brief

Make a scratch directory outside the repository with `mktemp -d`, then run:

    supersigil annotate prepare <session> --out <scratch>/brief.json <options>

It prints nothing when it succeeds. On an error, report it and stop: `no session matching` and
`matches several sessions` mean the person names the session again; `matches sessions in several
records` means they give a longer prefix, one that matches a single session; `no record for
<checkout>` means they run `supersigil session sync` first.

The brief is one line of JSON and can be many megabytes. Never paste the whole file into the
conversation. Read it in parts with `jq '<filter>' <scratch>/brief.json` (or `python3 -c` with the
`json` module when `jq` is missing), in this order:

1. The header: `del(.current, .corrections, .transcripts, .delegations, .restores, .discontinuities)`.
   You copy its `record`, `observation_revision` and `observed` into the document. `reconciled: false`
   means the record could not be brought up to date (`reconcile_note` says why); tell the person the
   brief holds what the record had.
2. `.current`: the session's current annotation as stored, `{revision, at, document}`, or null.
3. `.corrections`: the person's corrections of earlier revisions (step 4).
4. The transcripts: `[.transcripts[] | {transcript, agent_id, sidechain, turns: (.turns | length),
   capture_limitations}]`, then `.delegations, .restores, .discontinuities`.
5. The turns, 40 at a time: `.transcripts[0].turns[0:40]`, then `[40:80]`, until the main transcript
   (index 0) ends; then each agent transcript the same way. When a slice is too long, read fewer
   turns, or leave out patches with `| map(del(.edits[].patch))` and read one edit's patch when you
   need it.

Read each agent transcript after its dispatch. A `delegations` entry whose `step.type` is `start`
names the agent in `step.agent` and sits at `source_ordinal` of its `transcript`: the agent
transcript with that `agent_id` ran after that turn. A `stop` entry marks where the issuer learned
that the agent ended. No other order across transcripts is recorded; take none from times or
ordinals.

A turn has `id`, `sighting`, `role` (`human`, `agent`, `delegation`, `report`, `tool`, `meta` or
`summary`), `ordinal`, `text`, and the `edits` and `commands` recorded under it. `text` is
`{"state": "retained", "value": "…"}`, or the reason it was not kept: tool, meta and summary turns
keep no text, and thinking is never recorded. A rewritten transcript lists a turn id once per
sighting, possibly with different text. An edit has `id` (`edit:…`), `path`, `operation`, and
`patch` hunks or `new_text`. A command has `id` (`command:…`), `cmd`, `category`, `exit`, `outcome`
and output tails. In `restores`, each `edit` returned its file to the content it had before the
earlier edits listed in `restores`: those edits were undone.

## 3. Decisions

A decision is a choice the session carried out in edits, such as "Publish both stores through one
sync plan". A prompt, a test run or a restore is evidence about a decision, never a decision itself.
Fewer well-sourced decisions beat many guessed ones. A session may yield none; `decisions` is then
empty.

For each decision write:

- `id`: `d1`, `d2`, …, 1 to 64 characters of `A-Z`, `a-z`, `0-9`, `_` and `-`, unique among the
  decisions.
- `title`: the choice in one line, at most 200 characters.
- `cites`: the `edit:` and `command:` ids from the brief that carried it out. A review shows the
  decision on the lines those edits explain; a decision citing nothing in a change does not appear.
- `rationale`: why, one entry per reason. `text` is one plain line in your words, at most 2,000
  characters; the review prints its first line. `speaker` says who gave the reason:
  - `human`: the person, in a `human` turn;
  - `agent`: the agent, in an `agent` or `delegation` turn;
  - `annotator`: you, by inference, always with `"explicit": false`.

  `"explicit": true` needs a `source`, `{"turn": "<turn id>", "quote": "<words>"}`, whose quote of 1
  to 2,000 characters you copy verbatim from that turn's `text.value`: the same case, spacing and
  punctuation, with newlines written `\n`. Quote the words that give the reason; a quote proves the
  words were said, not that they explain these edits. A `report` turn is text the harness relayed,
  such as a task notification or another agent's result, not known to be the agent's words: it
  backs only an `annotator` line, which may name it as its `source`. A line that is not a quoted
  statement is an inference: `annotator`, `"explicit": false`, with or without a source. Import
  checks a `source` whatever `explicit` says: its quote must occur in the turn's retained text.
- `alternatives`: what the text proposed instead (`"status": "proposed"`, no `edits`) or what edits
  tried (`"status": "attempted"`, at least one edit), such as edits that `restores` shows undone. An
  optional `reason` has the shape of a rationale entry.
- `implications`: what the decision commits the code to, `{"text": "…", "explicit": false}`. An
  explicit one has a `source` quoting a `human`, `agent` or `delegation` turn.

Where the evidence leaves open something the reviewer should settle, add an open question,
`{"id": "q1", "text": "…", "about": ["edit:…"]}`, with ids unique among the questions. A question
becomes a gate item the person must judge when its `about` is empty or names an edit of the
reviewed change, so ask only what matters to the review.

## 4. Revising

When `current` is not null, start from `current.document`. Keep the id of every decision that still
stands; a new decision gets a new id.

Act on each entry of `corrections`. `addressed` holds the corrected decision's title, rationale and
cites in the revision the person corrected; `anchors_to` lists the ids of the current decisions the
correction applies to.

- `wrong`: `reason` says why the explanation is wrong. Re-examine the decisions in `anchors_to` and
  rewrite or drop the disputed rationale.
- `not_together`: the cited edits do not belong together. Split the decisions in `anchors_to` into
  decisions with new ids, each citing its own edits.
- `constraint`: `text` is the actual constraint. The review shows it ahead of the rationale, so
  leave it out of yours.

When `addressed` is null, `addressed_unavailable` says why; act on the `reason` or `text` as given.
Put every correction's `judgment` id in `corrections_seen`.

## 5. Import

Write the document to `<scratch>/annotation.json` in this shape:

```json
{
  "schema": "supersigil.annotation/1",
  "session": "<session id>",
  "based_on": 2,
  "prepared": { "record": "<record id>", "observation_revision": 17,
                "observed": { "count": 100, "last": "2026-10-15T14:02:11Z" } },
  "produced_by": "claude-code/supersigil-annotate",
  "corrections_seen": ["<judgment id>"],
  "decisions": [
    {
      "id": "d1",
      "title": "Publish both stores through one sync plan",
      "cites": ["edit:0123456789abcdef", "command:fedcba9876543210"],
      "rationale": [
        { "text": "…", "speaker": "human", "explicit": true,
          "source": { "turn": "<turn id>", "quote": "…" } },
        { "text": "…", "speaker": "annotator", "explicit": false }
      ],
      "alternatives": [
        { "title": "…", "status": "attempted", "edits": ["edit:…"],
          "reason": { "text": "…", "speaker": "agent", "explicit": true,
                      "source": { "turn": "<turn id>", "quote": "…" } } },
        { "title": "…", "status": "proposed" }
      ],
      "implications": [
        { "text": "…", "explicit": false }
      ]
    }
  ],
  "open_questions": [
    { "id": "q1", "text": "…", "about": ["edit:…"] }
  ]
}
```

`based_on` is `current.revision`, or null when `current` is null. `prepared` copies the brief's
`record`, `observation_revision` and `observed` unchanged. `produced_by` stays as shown. Lists may be
empty or left out, and an unknown field is refused. Then run:

    supersigil annotate import <scratch>/annotation.json --format terminal <options>

Success prints `Imported revision <r> of session <id> (<n> decisions, <m> open questions).` A refusal
imports nothing and prints `Annotation refused:` with one `<path>: <message>` line per problem. Fix
every listed path and import again, at most twice. The messages, and what fixes each:

- `schema: …, but this build reads supersigil.annotation/1`: set the schema shown above.
- `session: …, but this import is for session …` or `session … has no observations in this record`:
  copy `session` from the brief, and import with the options the brief was prepared with.
- `based_on: …, but the current revision is …`, or `Annotation based on revision …, but revision …
  is current; run annotate prepare again.`: an annotation was imported since your brief. Prepare
  again and revise from the new `current`.
- `prepared.record`, `prepared.observation_revision` or `prepared.observed.count`: copy `prepared`
  from the brief unchanged.
- `The annotation names record <id>, which does not exist.` or `The annotation names record <id>,
  which is not this checkout's or an ancestor's.`: the document's `prepared.record` is not a record
  these options reach. Prepare again with the same options and copy `prepared` from the new brief.
- `is blank` or `has <n> characters; at most <max>`: write a non-blank text within the limit.
- `is empty; an id has 1 to 64 characters`, `has <n> characters; an id has at most 64`,
  `"<id>" holds '<c>'; an id holds only A-Z, a-z, 0-9, _ and -`, or `"<id>" repeats
  decisions[<i>].id`: fix the id.
- `"<id>" is not an edit or command of this session`: use an `id` from some turn's `edits` or
  `commands`.
- `is missing; an explicit statement needs a source`: add the source, or set `"explicit": false`.
- `true, but speaker annotator is never explicit`: set `"explicit": false`.
- `"<turn>" is not a turn of this session`: use the `id` of the brief's turn whose `text.value`
  holds the quote.
- `not found verbatim in the retained text of turn "<turn>"`: copy the quote again from that turn's
  `text.value`, character for character.
- `the text of turn "<turn>" is not retained`: the turn keeps no text to quote. Point the `source`
  at a turn whose text is retained, holds the quote, and has a role that fits the speaker; or
  remove the `source`, then set `"speaker": "annotator"` and `"explicit": false` (an implication
  has no speaker: set only `"explicit": false`).
- `found only in <roles> text of turn "<turn>", but …`: the turn's role does not fit the speaker.
  Point the `source` at a turn whose role fits; or make the line an inference,
  `"speaker": "annotator"` and `"explicit": false` (only `"explicit": false` for an implication),
  and keep the `source`: any retained text backs an inference.
- `proposed, but it lists edits; …` or `attempted, but it lists no edits; …`: match `status` to
  `edits`.
- `"<id>" is not a judgment id` in `corrections_seen`: copy the `judgment` id from `corrections`.
- `has <n> entries; at most <max>`: shorten the list.
- `The annotation is not a valid document: <path>: <message>`: the document is not of this shape at
  `<path>`, such as `decisions[0].rationale[0].source.weight`: a wrong type, an unknown field, or a
  JSON syntax error, which may print no path. Only the first such problem is printed, so fix it and
  import again.

Report the result to the person: the imported revision, each decision's id and title, and each open
question; or, when the second retry is refused too, the remaining problems and that nothing was
imported.
