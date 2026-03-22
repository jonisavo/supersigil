---
supersigil:
  id: inventory-queries/req
  type: requirements
  status: implemented
title: "CLI Inventory Queries"
---

## Introduction

This spec recovers the CLI domain for exploring workspace inventory rather than
current work state: `ls`, `schema`, and `graph`.

It captures the current table-based listing output, config-only schema export,
and document-graph visualization behavior as implemented today. It does not
re-spec `context`, `plan`, `verify`, `status`, `affected`, `init`, `new`, or
`import`.

## Definitions

- **Inventory_Query_Command**: One of `ls`, `schema`, or `graph`.
- **Listing_Filter**: An optional `ls` filter over document type, document
  status, or project membership.
- **Schema_Output**: The merged component-definition and document-type schema
  serialized by `schema`.
- **Graph_View**: The Mermaid or Graphviz DOT rendering emitted by `graph`.

## Requirement 1: Document Inventory Listing

As a developer, I want to inspect the current document inventory with optional
filters, so that I can quickly see what specs exist in the workspace or one
project slice.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-1-1">
    THE `ls` command SHALL load the graph, collect all documents, apply the
    optional `--type`, `--status`, and `--project` Listing_Filters
    conjunctively, and sort the resulting entries by document ID before
    output.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-cli/src/commands/ls.rs" />
  </Criterion>
  <Criterion id="req-1-2">
    In terminal mode, THE `ls` command SHALL render an aligned table with the
    columns `ID`, `Type`, `Status`, and `Path`. Displayed paths SHALL be
    project-root-relative when possible, and the output SHALL end with a
    document count footer.
  </Criterion>
  <Criterion id="req-1-3">
    IF the filtered result set is empty, THEN terminal `ls` output SHALL print
    `No documents found.` and the command SHALL still succeed.
  </Criterion>
  <Criterion id="req-1-4">
    In JSON mode, THE `ls` command SHALL write an array of entries containing
    `id`, `path`, and optional `doc_type` and `status` fields.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 2: Schema Export

As an agent or developer, I want to inspect the current authoring schema
without depending on the parseability of the spec corpus, so that I can learn
valid components and document types even in a broken workspace.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-2-1">
    THE `schema` command SHALL load config directly, merge built-in component
    definitions with configured component overrides and additions, merge built-in
    document types with configured document-type definitions, and emit that
    Schema_Output without requiring spec-file parsing to succeed.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-cli/tests/cmd_schema.rs" />
  </Criterion>
  <Criterion id="req-2-2">
    THE `schema` command SHALL support `--format json|yaml`, with YAML as the
    default format.
  </Criterion>
  <Criterion id="req-2-3">
    Schema_Output component entries SHALL include the current description,
    attribute definitions, referenceable flag, verifiable flag,
    `target_component`, and examples when those fields are present. Document
    type entries SHALL include description, valid statuses, and required
    components when present.
  </Criterion>
  <Criterion id="req-2-4">
    Schema_Output serialization SHALL omit false or empty fields rather than
    emitting placeholder noise for absent descriptions, examples, attributes,
    or optional flags.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 3: Graph Visualization

As a developer, I want to visualize the current document graph, so that I can
inspect dependency structure and traceability relationships directly from the
CLI.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-3-1">
    THE `graph` command SHALL load the graph and emit a Graph_View with one node
    per document and one edge per resolved top-level ref-bearing component.
    Node labels SHALL include the document ID and current document type when
    present, and edge labels SHALL use the emitting component name.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-cli/src/commands/graph.rs" />
  </Criterion>
  <Criterion id="req-3-2">
    THE `graph` command SHALL support `--format mermaid|dot`, with Mermaid as
    the default format.
  </Criterion>
  <Criterion id="req-3-3">
    Graph_View syntax SHALL be written to stdout only. The command SHALL write
    a node-and-edge summary plus a pipe-to-file hint to stderr so redirected
    graph output stays clean.
  </Criterion>
</AcceptanceCriteria>
```
