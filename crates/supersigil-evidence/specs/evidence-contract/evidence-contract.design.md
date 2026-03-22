---
supersigil:
  id: evidence-contract/design
  type: design
  status: approved
title: "Evidence Contract"
---

```supersigil-xml
<Implements refs="evidence-contract/req" />
<TrackedFiles paths="crates/supersigil-evidence/src/lib.rs, crates/supersigil-evidence/src/types.rs, crates/supersigil-evidence/src/provenance.rs, crates/supersigil-evidence/src/plugin.rs, crates/supersigil-evidence/src/tests.rs" />
```

## Overview

`evidence-contract` is the shared normalized evidence layer.

The important current boundary is that this crate defines shared data and trait
contracts only. It does not parse source code, load config, or run
verification rules.

That keeps ecosystem-specific discovery in plugin crates and merge/report logic
in `supersigil-verify` while still giving them one serializable contract.

## Architecture

```mermaid
graph TD
    TYPES["types.rs"]
    PROV["provenance.rs"]
    PLUGIN["plugin.rs"]
    LIB["lib.rs re-exports"]
    VERIFY["supersigil-verify"]
    RUST["supersigil-rust"]

    TYPES --> LIB
    PROV --> LIB
    PLUGIN --> LIB
    LIB --> VERIFY
    LIB --> RUST
```

## Module Boundaries

### `types.rs`

Owns the shared serializable primitives:

- `EvidenceId`
- `SourceLocation`
- `VerifiableRef`
- `VerificationTargets`
- `TestKind`
- `TestIdentity`
- `EvidenceKind`
- `VerificationEvidenceRecord`
- `ProjectScope`

The notable current constraint is that both `VerifiableRef` and
`VerificationTargets` are criterion-oriented. The shared contract intentionally
has no document-level or empty-target evidence shape.

### `provenance.rs`

Owns:

- `PluginProvenance`
- `EvidenceConflict`

These types let downstream consumers preserve where evidence came from without
embedding plugin-specific logic into the record shape itself.

### `plugin.rs`

Owns:

- `PluginDiagnostic`
- `PluginDiscoveryResult`
- `PluginError`
- `EcosystemPlugin`

The current plugin trait depends on `DocumentGraph` from `supersigil-core`,
keeps `ProjectScope` minimal, gives plugins an explicit place to plan
plugin-owned discovery inputs from shared resolved test files, and returns
already-normalized evidence plus non-fatal diagnostics through
`PluginDiscoveryResult`.

## Re-Export Surface

`lib.rs` re-exports the public contract directly:

```rust
pub use plugin::{EcosystemPlugin, PluginDiagnostic, PluginDiscoveryResult, PluginError};
pub use provenance::{EvidenceConflict, PluginProvenance};
pub use types::{
    EvidenceId, EvidenceKind, ProjectScope, SourceLocation, TestIdentity, TestKind,
    VerifiableRef, VerificationEvidenceRecord, VerificationTargets,
};
```

This keeps downstream crates on a flat public API even though the
implementation is split across three modules.

## Testing Strategy

- `crates/supersigil-evidence/src/tests.rs`
  covers public-type behavior such as `VerifiableRef::parse`, stable string
  conversions, equality semantics, record construction, provenance variants,
  and the plugin trait/error surface.

## Current Gaps

- Plugin diagnostics are currently warning-only and do not yet carry richer
  structured positions beyond an optional file path.
- `ProjectScope` is intentionally minimal and does not encode ecosystem-
  specific discovery configuration.
