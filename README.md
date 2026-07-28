# sema-translator

`sema-translator` is the sole-writer authority for exact spelling ↔
variant-fronted encodedID-chain correspondence. It owns nested,
module-scoped name tables, allocates declarations, resolves references, performs
identity-preserving operational rename, and publishes append-only Rust
vocabulary releases.

It does not store Ethos, Nomos, Logos, Spirit, Orchestrate, Mind, Messenger, or
other component documents. It has no compatibility path to `sema-storage`.

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
