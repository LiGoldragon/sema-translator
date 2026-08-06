# Architecture

`sema-translator` has one surface: it turns an authority-approved naming
transition into a verified bootstrap assembly for strict Ethos input.

```text
prepared Ethos draft
        +
explicit authority identity seats
        |
        v
BootstrapTransactionAssembler
        |
        +-- exact transition and seat validation
        +-- authority-bound receipt validation
        +-- read-only verified name projection
        |
        v
VerifiedBootstrapAssembly
```

The caller supplies identities that already exist in the naming authority.
`SemaBootstrapNamingAuthority` neither mints identities nor derives them from
spelling or content. It approves only the configured before/after metadata
transition, the exact set of new canonical identity bytes, and the exact
generated stream seats. The reader exposes a resolver only after that complete
transaction validates.

`core-ethos` owns planning and prepared bootstrap transaction shapes.
`signal-sema-translator` owns the encoded vocabulary identities used at the
boundary. `sema-translator` supplies the authority proof and verified assembly.
`sema-engine` owns runtime execution and persistence.

The `bootstrap` feature is the default and sole crate surface. There is no
runtime feature, engine dependency, database, actor, daemon, store, socket, or
wire route in this repository.
