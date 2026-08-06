# sema-translator

`sema-translator` is the sole-writer naming authority. Its existing daemon owns
the nested exact-spelling tables, resolves references, performs
identity-preserving operational rename, and publishes append-only Rust
vocabulary releases. Its production bootstrap module separately authenticates
explicit authority-approved opaque identity seats for the renewed Ethos reader.

It does not store Ethos, Nomos, Logos, Spirit, Orchestrate, Mind, Messenger, or
other component documents. It has no compatibility path to `sema-storage`.

## Bootstrap transaction assembly

`sema_translator::bootstrap` is the source-to-transaction authority boundary for
the provisional Interface, Nexus, and Sema bootstrap readers. A caller supplies
one complete authority approval:

- the exact textual-metadata after snapshot;
- canonical bytes for every already-minted new opaque identity;
- exact initiation and termination identities for every authored Stream.

The module plans the source through `core-ethos`, matches declarations to exact
metadata addresses, distinguishes Existing from New against the configured
before snapshot, and refuses missing or unused seats. It never derives an
identity from spelling or content and has no allocation fallback. The injected
`SemaBootstrapNamingAuthority` issues a receipt only for the exact approved
draft. Canonical source writing revalidates that receipt and the complete
prepared model before `VerifiedBootstrapResolver` can be constructed.

The resulting `VerifiedBootstrapAssembly` keeps together the matching reader,
authority-branded `PreparedBootstrapTransaction`, canonical source, and
post-validation resolver required by Nomos and Rust Logos. This is transient
assembly and verification; it does not put component documents in the naming
authority database and does not translate through the historical six-slot
schema model or the spelling-derived nested allocator.

## Runtime surfaces

- Rust library: `sema_translator`
- daemon binary: `sema-translator-daemon`
- service name reserved for deployment: `sema-translator-daemon.service`
- runtime directory: `sema-translator`
- socket basename: `sema-translator.sock`
- owned database basename: `sema-translator.sema`
- wire contract: `signal_sema_translator`, contract 4 / revision 1
- translator archive: version 1

The daemon requires every runtime path and authorized Unix UID explicitly:

```sh
sema-translator-daemon daemon \
  --socket /run/sema-translator/sema-translator.sock \
  --database /var/lib/sema-translator/sema-translator.sema \
  --authorized-uid 991
```

There are no `/tmp` defaults, old socket aliases, redirects, adapters, or
fallbacks.

## Wired now

- one actor exclusively owns `sema_engine::Engine` and its `.sema` file;
- a fresh store bootstraps mutable Universal priors `Integer` and `String`,
  plus immutable Rust prior `u64`;
- Universal seals, operational renames, and trusted Rust vocabulary releases
  commit the complete versioned name-table archive and an external
  idempotency receipt atomically;
- expected database markers and exact table generations reject stale writers;
- immutable table refusal occurs before target lookup;
- committed replies can be recovered by operation key after a lost response;
- verified current and historical snapshots remain readable;
- contract and revision binding is checked before archived request bodies;
- the kernel-authenticated Unix peer UID is domain-mapped to a typed principal,
  then checked against the exact claimed role/capability;
- startup verifies full versioned commit ancestry and requires one immutable
  external receipt for every post-bootstrap state transition;
- unknown contract-local routes and live-socket replacement are refused;
- post-commit events are emitted only for newly committed mutations.
- authority-approved bootstrap source can be sealed into a branded transaction
  with a resolver that is unavailable before receipt validation.

Every Git dependency is pinned to a published full revision:

- `signal-sema-translator`
  `5df821a335bdbd28582f71d4f25d4bd31deee567`
- `name-table` `c3a4b472caa6d225bb9eb09b1893942cdb5fec9e`
- `sema-engine` `94d2f7ee7b81d0bf3b6f1a111f6bbbc398c0e7a3`
- `signal-frame` `0786fbe8caf27552afcdd5deb85bc82ec6088337`
- the contract pins `protos`
  `0b4b17471053e8b40472225b5992cd3252e76d85`

## Deliberately future

Service-manager packaging and policies with finer capability division than one
authorized Unix service account are not chosen here. The library policy
interface permits those deployments while the production listener always
authenticates the kernel-owned peer UID.

The emitted Rust chain encoding, move, retirement, Capsule pin composition,
dynamic-enum member identity, future language roots, and the larger
human-language word space remain outside this repository. No stored or wire
shape anticipates them.

## Validation

```sh
cargo test
nix flake check -L
```

The process witnesses use temporary sockets and `.sema` files only.
