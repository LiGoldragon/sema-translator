# sema-translator

Authority-approved bootstrap translation for strict Ethos assembly.

The crate matches explicit, already-minted identity seats to one complete
prepared draft, validates the exact authority transition, and returns a
verified bootstrap assembly with a read-only name projection. It never derives
identity from spelling or content and never allocates on behalf of a reader.

The `bootstrap` feature is both the default and the only surface. Runtime
execution and persistence belong to `sema-engine`; this repository contains no
engine, actor, database, daemon, store, socket, or wire service.

The principal types are:

- `AuthorizedBootstrapTransition`
- `SemaBootstrapNamingAuthority`
- `BootstrapTransactionAssembler`
- `VerifiedBootstrapAssembly`
- `VerifiedBootstrapResolver`

Run the complete proof with:

```sh
cargo test --all-targets --all-features
cargo clippy --all-targets --all-features -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --all-features --no-deps
nix flake check -L
```
