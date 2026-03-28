---
supersigil:
  id: rename/tasks
  type: tasks
  status: done
title: "LSP: Rename"
---

```supersigil-xml
<DependsOn refs="rename/design" />
```

## Overview

Implementation sequence: enrich cursor detection first, then rename target
detection, then edit collection, then LSP handler wiring. Each task is
independently testable.

```supersigil-xml
<Task
  id="task-1"
  status="done"
  implements="rename/req#req-1-1, rename/req#req-1-2, rename/req#req-2-2, rename/req#req-2-3"
>
  Enrich `find_ref_at_position()` in `definition.rs` to return
  `Option&lt;RefAtPosition&gt;` instead of `Option&lt;String&gt;`. The new struct
  includes `ref_string`, `part` (DocId or Fragment), `part_start`, and
  `part_end` byte offsets. Update the three existing callers in `state.rs`,
  `references.rs`, and `hover.rs` to use `.ref_string`. Add unit tests for
  the new part and span fields.
</Task>

<Task
  id="task-2"
  status="done"
  implements="rename/req#req-1-1, rename/req#req-1-2, rename/req#req-1-3, rename/req#req-1-4, rename/req#req-1-5, rename/req#req-1-6, rename/req#req-1-7, rename/req#req-2-1"
  depends="task-1"
>
  Implement `find_rename_target()` in a new `rename.rs` module. Define
  `LineRange` and `RenameTarget` types. Four detection strategies in priority
  order: ref attribute (via enriched `find_ref_at_position`), supersigil-ref
  info string, component tag / id attribute value, frontmatter. Add
  `mod rename` to `lib.rs`. Unit tests for each detection case, priority
  ordering, and rejection of non-renameable positions.
</Task>

<Task
  id="task-3"
  status="done"
  implements="rename/req#req-3-1, rename/req#req-3-2, rename/req#req-3-3, rename/req#req-3-4, rename/req#req-3-5"
  depends="task-2"
>
  Implement `collect_rename_edits()` in `rename.rs`. For ComponentId renames:
  update definition site id attribute, ref attribute fragments across all
  documents, supersigil-ref tokens, and task implements entries. For
  DocumentId renames: update frontmatter id value, doc ID portions in ref
  attributes, and task implements entries. Group edits by URI into a
  WorkspaceEdit. Convert byte offsets to UTF-16. Integration tests using
  constructed DocumentGraph instances.
</Task>

<Task
  id="task-4"
  status="done"
  implements="rename/req#req-4-1, rename/req#req-4-2"
  depends="task-2"
>
  Implement new-name validation in the rename handler: reject empty names,
  names with whitespace, `#`, or `"`. Return a ResponseError with a
  descriptive message on failure. Unit tests for each validation rule.
</Task>

<Task
  id="task-5"
  status="done"
  implements="rename/req#req-5-1"
  depends="task-3, task-4"
>
  Wire the handlers in `state.rs`: add `rename_provider` with
  `prepare_provider: true` to ServerCapabilities. Implement
  `fn prepare_rename()` returning `PrepareRenameResponse::RangeWithPlaceholder`.
  Implement `fn rename()` calling `find_rename_target`, validation, and
  `collect_rename_edits`. Follow the same async pattern as existing handlers.
</Task>
```
