---
supersigil:
  id: document-symbols/tasks
  type: tasks
  status: done
title: "LSP Document Symbols"
---

```supersigil-xml
<DependsOn refs="document-symbols/design" />
```

## Overview

Three tasks: parser prerequisite (end positions), symbol mapping module,
and LSP integration.

```supersigil-xml
<Task
  id="task-1"
  status="done"
  implements="document-symbols/req#req-1-5"
>
  Add `end_position: SourcePosition` to `ExtractedComponent`. In the
  XML parser, capture the end offset from `Event::End` and
  `Event::Empty` events. In `xml_extract.rs`, compute `end_position`
  from the end offset using `line_col()`. Add parser tests verifying
  end positions for regular and self-closing elements.
</Task>

<Task
  id="task-2"
  status="done"
  implements="document-symbols/req#req-1-1, document-symbols/req#req-1-2, document-symbols/req#req-1-3, document-symbols/req#req-1-4, document-symbols/req#req-3-1, document-symbols/req#req-3-2"
  depends="task-1"
>
  Create `document_symbols.rs` in `supersigil-lsp` with a
  `document_symbols(doc, content)` function that maps
  `ExtractedComponent` trees to `DocumentSymbol` trees. Implement kind
  mapping, name/detail logic, range conversion, and recursive children.
  Handle empty documents and parse-error partial results. Add unit tests.
</Task>

<Task
  id="task-3"
  status="done"
  implements="document-symbols/req#req-2-1, document-symbols/req#req-2-2"
  depends="task-2"
>
  Wire `textDocument/documentSymbol` into the LSP server: add
  `documentSymbolProvider` to `ServerCapabilities` (gated on config
  presence), add the request handler in `state.rs`, register the module
  in `lib.rs`. Verify the full flow with `supersigil lint` and
  `cargo nextest run`.
</Task>
```
