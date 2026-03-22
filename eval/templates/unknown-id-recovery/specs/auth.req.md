---
supersigil:
  id: auth/req/login
  type: requirements
  status: approved
title: "Login Requirement"
---

# Login Requirement

<AcceptanceCriteria>
  <Criterion id="valid-creds">
    WHEN valid email and password are submitted, THEN the system SHALL return a session token.
  </Criterion>
  <Criterion id="lockout">
    WHEN repeated invalid logins occur, THEN the system SHALL require a lockout response.
  </Criterion>
</AcceptanceCriteria>

