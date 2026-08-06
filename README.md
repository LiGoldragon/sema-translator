# sema-translator

Sema-owned bootstrap identity authority for strict Ethos assembly.

Callers provide only source text and `SourcePlacement` to `authorize_bootstrap`.
Sema privately owns the authority instance, mints opaque `EncodedName` values
with a CSPRNG, derives the direct
`TrueName` of every strict declaration value, and stages canonical textual
metadata plus the authority receipt. Replaying an already realized request does
not mint again; a distinct request waits for the later atomic persistence owner.
Bundled/generated Stream declarations are refused during planning, before any
identity allocation or stage is created.

The `bootstrap` feature is both the default and the only surface. Runtime
execution and persistence belong to `sema-engine`; this repository contains no
engine, actor, database, daemon, store, socket, or wire service.

The public authority surface is:

- `authorize_bootstrap`
- `SourcePlacement`
- opaque `AuthorizedBootstrap`

Run the complete proof with:

```sh
cargo test --all-targets --all-features
cargo clippy --all-targets --all-features -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --all-features --no-deps
```
