---
supersigil:
  id: auth/tasks/login
  type: tasks
  status: draft
title: "Login Tasks"
---

# Login Tasks

```supersigil-xml
<Task id="task-1-1" implements="auth/req/login#valid-creds">
  Implement the happy-path login flow.
</Task>

<Task id="task-1-2" implements="auth/req/login#lockout" depends="task-1-1">
  Add lockout handling after the happy path is in place.
</Task>
```
