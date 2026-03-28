---
supersigil:
  id: lsp-code-actions/tasks
  type: tasks
  status: done
title: "LSP Code Actions / Quick Fixes — Tasks"
---

```supersigil-xml
<DependsOn refs="lsp-code-actions/design" />
```

## Overview

Implementation sequence: foundation types first (diagnostic data,
provider trait), then the handler, then providers in order of
complexity (simplest first), then the create-document interactive
flow, then shared scaffolding extraction. Testing is interleaved
with each task via TDD.

```supersigil-xml
<Task id="task-1" status="done" implements="lsp-code-actions/req#req-1-1, lsp-code-actions/req#req-1-2, lsp-code-actions/req#req-1-3, lsp-code-actions/req#req-1-4">
  Define DiagnosticData, DiagnosticSource, ParseDiagnosticKind,
  GraphDiagnosticKind, and ActionContext types in diagnostics.rs.
  Add Serialize/Deserialize derives. Update finding_to_diagnostic(),
  parse error, and graph error conversion functions to attach
  DiagnosticData to the diagnostic data field.
</Task>
```

```supersigil-xml
<Task id="task-2" status="done" depends="task-1" implements="lsp-code-actions/req#req-3-1, lsp-code-actions/req#req-3-2, lsp-code-actions/req#req-3-3">
  Define CodeActionProvider trait and ActionRequestContext struct
  in a new code_actions.rs module. Add provider registration to
  SupersigilLsp initialization (empty Vec initially).
</Task>
```

```supersigil-xml
<Task id="task-3" status="done" depends="task-2" implements="lsp-code-actions/req#req-2-1, lsp-code-actions/req#req-2-2, lsp-code-actions/req#req-2-3, lsp-code-actions/req#req-2-4">
  Implement the textDocument/codeAction handler in state.rs.
  Advertise codeActionProvider with quickfix kind in capabilities.
  Deserialize DiagnosticData from each diagnostic, iterate
  providers, collect and return CodeActions.
</Task>
```

```supersigil-xml
<Task id="task-4" status="done" depends="task-3" implements="lsp-code-actions/req#req-4-2">
  Implement MissingAttributeProvider. Insert the missing required
  attribute with a placeholder value at the component's opening tag.
  Add insta snapshot tests.
</Task>
```

```supersigil-xml
<Task id="task-5" status="done" depends="task-3" implements="lsp-code-actions/req#req-4-3">
  Implement DuplicateIdProvider. Offer to rename the duplicate ID
  by appending a numeric suffix. Add insta snapshot tests.
</Task>
```

```supersigil-xml
<Task id="task-6" status="done" depends="task-3" implements="lsp-code-actions/req#req-4-4">
  Implement IncompleteDecisionProvider. Insert stub Rationale or
  Alternative inside the Decision. Add insta snapshot tests.
</Task>
```

```supersigil-xml
<Task id="task-7" status="done" depends="task-3" implements="lsp-code-actions/req#req-4-5">
  Implement MissingComponentProvider. Insert a skeleton of the
  required component at the appropriate location. Add insta
  snapshot tests.
</Task>
```

```supersigil-xml
<Task id="task-8" status="done" depends="task-3" implements="lsp-code-actions/req#req-4-6">
  Implement OrphanDecisionProvider. Add a References component
  with refs pointing to the parent document. Add insta snapshot
  tests.
</Task>
```

```supersigil-xml
<Task id="task-9" status="done" depends="task-3" implements="lsp-code-actions/req#req-4-7">
  Implement InvalidPlacementProvider. Move the misplaced component
  to the correct parent. Add insta snapshot tests.
</Task>
```

```supersigil-xml
<Task id="task-10" status="done" depends="task-3" implements="lsp-code-actions/req#req-4-8">
  Implement SequentialIdProvider. Renumber component IDs to restore
  sequential order. Add insta snapshot tests.
</Task>
```

```supersigil-xml
<Task id="task-11" status="done" depends="task-3" implements="lsp-code-actions/req#req-4-1, lsp-code-actions/req#req-5-1">
  Implement BrokenRefProvider — the non-interactive path. Offer
  "remove broken ref" (edit attribute) and "create document" when
  the target path is unambiguous (direct WorkspaceEdit with
  CreateFile). Add insta snapshot tests.
</Task>
```

```supersigil-xml
<Task id="task-12" status="done" depends="task-11" implements="lsp-code-actions/req#req-5-4">
  Extract the supersigil new template logic into a shared function
  accessible to both CLI and LSP. Update the CLI new command to
  call the shared function.
</Task>
```

```supersigil-xml
<Task id="task-13" status="done" depends="task-11, task-12" implements="lsp-code-actions/req#req-5-2, lsp-code-actions/req#req-5-3, lsp-code-actions/req#req-5-5">
  Implement the interactive create-document flow. Add the
  supersigil.createDocument command handler. Use
  window/showMessageRequest for project selection. Resolve spec
  dir from the chosen project's glob prefix. Apply via
  workspace/applyEdit. Handle dismissal gracefully.
</Task>
```

```supersigil-xml
<Task id="task-14" status="done" depends="task-4, task-5, task-6, task-7, task-8, task-9, task-10, task-11, task-13" implements="lsp-code-actions/req#req-6-1, lsp-code-actions/req#req-6-2">
  Add the format_actions snapshot helper and integration tests that
  apply WorkspaceEdits to files and re-verify that diagnostics are
  resolved. Ensure all providers have snapshot coverage.
</Task>
```
