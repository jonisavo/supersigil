---
supersigil:
  id: auth/req/login
  type: requirements
  status: approved
title: "Login Requirement"
---

# Login Requirement

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="happy-path-login">
    WHEN valid email and password are submitted, THEN the system SHALL create a session.
  </Criterion>
</AcceptanceCriteria>
```
