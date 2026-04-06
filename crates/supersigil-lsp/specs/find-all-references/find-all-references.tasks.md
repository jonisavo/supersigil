---
supersigil:
  id: find-all-references/tasks
  type: tasks
  status: done
title: "LSP: Find All References"
---

```supersigil-xml
<DependsOn refs="find-all-references/design" />
```

## Overview

Implementation sequence: graph accessor methods first, then cursor detection,
then reference collection, then LSP handler wiring. Each task is independently
testable.

```supersigil-xml
<Task
  id="task-1"
  status="done"
  implements="find-all-references/req#req-2-1, find-all-references/req#req-2-2"
>
  Add graph accessor methods to `DocumentGraph` in `supersigil-core`:
  - Promote `resolve_component_path` from `reverse.rs` to public
    `component_at_path(doc_id, path)`.
  - Add `resolved_refs_for_doc(doc_id)` to iterate all resolved refs
    originating from a document.
  - Add `task_implements_for_doc(doc_id)` to iterate all task implements
    entries from a document.
  Unit tests for each accessor.
</Task>

<Task
  id="task-2"
  status="done"
  implements="find-all-references/req#req-1-1, find-all-references/req#req-1-2, find-all-references/req#req-1-3, find-all-references/req#req-1-4"
  depends="task-1"
>
  Implement cursor detection in `references.rs`:
  `find_reference_target(content, line, character, doc_id, graph)`.
  Three strategies in priority order: ref string (reuse
  `find_ref_at_position`), component definition tag with `id` attribute,
  frontmatter. Unit tests for each detection case and priority ordering.
</Task>

<Task
  id="task-3"
  status="done"
  implements="find-all-references/req#req-2-1, find-all-references/req#req-2-2, find-all-references/req#req-2-3, find-all-references/req#req-2-4, find-all-references/req#req-2-5"
  depends="task-1"
>
  Implement reference collection in `references.rs`:
  `collect_references(target_doc, target_fragment, include_declaration, graph)`.
  Query reverse mappings for source doc IDs, scan resolved_refs and
  task_implements for positions, convert to Locations. Handle
  includeDeclaration, empty results, and unknown targets.
  Unit tests with constructed DocumentGraph instances.
</Task>

<Task
  id="task-4"
  status="done"
  implements="find-all-references/req#req-3-1"
  depends="task-2, task-3"
>
  Wire the handler in `state.rs`: add `references_provider` to
  ServerCapabilities, implement `fn references()` following the same
  async pattern as `fn definition()`. Add `mod references` to `lib.rs`.
</Task>
```
