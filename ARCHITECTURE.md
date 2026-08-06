# Architecture

`sema-translator` has one surface: it owns naming authority for strict Ethos
source plus source placement.

```text
source text + SourcePlacement
        |
        v
empty SemaBootstrapAuthority handle
        |
        +-- allocation-free plan (refuses bundled Stream)
        +-- private CSPRNG `EncodedName` allocation
        +-- canonical `TextualMetadataRecord` transition
        +-- receipt and direct strict-value `TrueName` stage
        |
        v
opaque AuthorizedBootstrap
```

The caller cannot provide or construct identities, proof, receipt, seat,
catalog, canonical bytes, or transaction. The authority resolves an exact live
textual projection address to its existing opaque name, or mints a new one.
Conflicting occupied addresses and implicit rename/reparent attempts are typed
refusals. The plan occurrence is a local phase join only and never persists.

`core-ethos` owns planning and prepared bootstrap transaction shapes plus the
identity-free prior vocabulary seed. `sema-translator` owns allocation,
metadata, replay, and staging. The later persistence owner performs the atomic
durable transition; `sema-engine` owns runtime execution.

The `bootstrap` feature is the default and sole crate surface. There is no
runtime feature, engine dependency, database, actor, daemon, store, socket, or
wire route in this repository.
