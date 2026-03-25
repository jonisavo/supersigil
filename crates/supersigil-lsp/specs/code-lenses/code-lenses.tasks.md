---
supersigil:
  id: code-lenses/tasks
  type: tasks
  status: done
title: "LSP: Code Lenses"
---

```supersigil-xml
<DependsOn refs="code-lenses/design" />
```

## Overview

Implementation sequence: evidence cache first (needed by all lenses), then
the pure lens-building function with tests, then LSP handler wiring. Each
task is independently testable.

```supersigil-xml
<Task
  id="task-1"
  status="done"
  implements="code-lenses/req#req-5-1"
>
  Cache evidence index in server state. Add an `evidence_by_target` field
  to `SupersigilLsp` in `state.rs`. Populate it in `run_verify_and_publish`
  by cloning the `ArtifactGraph`'s `evidence_by_target` secondary index.
  Add `code_lens_provider` to `ServerCapabilities` in `initialize`.
</Task>

<Task
  id="task-2"
  status="done"
  implements="code-lenses/req#req-1-1, code-lenses/req#req-1-2, code-lenses/req#req-1-3, code-lenses/req#req-1-4, code-lenses/req#req-1-5, code-lenses/req#req-2-1, code-lenses/req#req-2-2, code-lenses/req#req-3-1, code-lenses/req#req-3-2, code-lenses/req#req-3-3, code-lenses/req#req-3-4, code-lenses/req#req-3-5, code-lenses/req#req-4-1, code-lenses/req#req-4-2"
  depends="task-1"
>
  Implement `build_code_lenses` in `code_lens.rs`. Single pure function
  that walks the document component tree and frontmatter, producing
  `Vec&lt;CodeLens&gt;` with all three lens types (Document,
  AcceptanceCriteria, Criterion). Includes reference count aggregation,
  coverage computation, verification status formatting, and click action
  commands. Unit tests for all formatting variants, positions, scoped
  AcceptanceCriteria coverage, click actions, and behavior with/without
  verify data.
</Task>

<Task
  id="task-3"
  status="done"
  implements="code-lenses/req#req-5-1"
  depends="task-2"
>
  Wire the `textDocument/codeLens` handler in `state.rs`. Look up the
  document from `file_parses` (falling back to `partial_file_parses`),
  read buffer content from `open_files`, resolve the doc ID, and
  delegate to `build_code_lenses`. Add `mod code_lens` to `lib.rs`.
</Task>
```
