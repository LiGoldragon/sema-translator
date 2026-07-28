# sema-translator architecture

## Boundary

`sema-translator` is one stateful daemon with one owned embedded SEMA database.
It is the encodedID-chain ↔ exact-spelling authority. Component engines keep
their documents in their own databases.

```text
clients
  │ signal-sema-translator contract 4 / wire revision 1
  ▼
bound Unix socket
  │ kernel peer UID → typed principal + checked claim
  ▼
one authority actor
  │ pure name-table staging
  ▼
sema-engine
  ▼
sema-translator.sema
```

The actor is the only database owner and serializes every request. There is no
distributed transaction: a translator commit is independently durable. A
client crash after that commit is recovered through the operation receipt.

## Stored family

One versioned SEMA record family contains:

- exactly one authority-state row;
- one immutable external receipt row per committed client mutation.

The state row carries:

- archive marker `SEMATR01` and explicit archive version 1;
- exact `SSTA` root records for `VocabularyRoot::{Universal, Rust}`;
- the complete integrity-protected name-table archive;
- the monotonic trusted Rust release ledger.

Name-table heads, snapshots, allocation cursors, and generic receipts are
inside the complete archive. The external receipt adds the
contract-level request digest, operation result, Rust release version where
applicable, and exact embedded database marker.

The state row and new external receipt row land through one heterogeneous
`sema-engine` commit. The daemon predicts both next marker coordinates from
the actor-owned current marker, stores that prediction, and verifies the
returned commit marker byte-for-byte before updating memory or answering.
Startup independently checks every external receipt marker against the SEMA
commit log and requires its state and receipt record keys in the same commit.
Exactly one receiptless state commit is permitted: the fixed bootstrap. Every
later state commit is exactly one state mutation plus one newly asserted
receipt. A receipt-only change, removed receipt, repeated receipt key, foreign
family, discontinuous marker, or disagreement between metadata and versioned
logs prevents readiness. Startup refolds the authoritative versioned log,
which verifies the full digest chain before materialized records are trusted.

`Engine::open_recovering` runs before the socket binds. Startup then validates
the archive envelope, both explicit roots, complete name-table archive and
snapshot integrity, table ancestry, generic receipts, Rust release monotonicity,
external receipt keys, markers, commit ancestry, and referenced historical
snapshots. Any inconsistency prevents readiness.

Legacy flat archives, the central storage archive, unknown roots, and
unsupported archive versions have no migration or inference path.

## Initial authority

A virgin database receives one trusted internal bootstrap:

```text
Universal (mutable)  0 ↔ Integer
                     1 ↔ String

Rust (immutable)     0 ↔ u64
```

The bootstrap shape is fixed in code. No caller supplies mutability. Universal
builtins remain ordinary mutable entries whose operational rename is protected
by authorization. Rust tables remain spelling-immutable.

## Writes

`SealUniversal` lowers only to mutable Universal tables. Declarations allocate
or reuse exact spellings; references only resolve. Parent tables are staged
before child tables. Same-table redefinition, an unresolved reference, or any
other failure discards the entire staged graph.

Fresh entries use name-table's canonical exact-byte allocation ordering. This
makes an identical declaration set allocate identical IDs and produce an
identical digest regardless of traversal order.

The allocation behavior is documented at its generic allocation site:
changing authored text introduces an unseen spelling, receives a new ID, and
leaves the previous row allocated. Only `Rename` preserves identity.

`Rename` targets one complete chain. Name-table loads the owning head and
checks table mutability before target lookup. A successful rename changes one
spelling, advances that table's immutable generation, and leaves the target,
child-table address, and all descendant chains unchanged.

`PublishRustVocabulary` is separately authorized and monotonically versioned.
It can resolve existing Rust spellings and append new ones. Its DTO and
immutable table policy cannot express alteration, removal, rebinding, or
rename.

Idempotency lookup precedes optimistic-state checks. Replaying the same key and
semantic digest returns the original receipt and marker even after later
commits. Reusing a key with different content fails without a write.

## Reads and events

Revision-1 reads return the current marker, exact heads, current snapshots,
verified historical snapshots, or committed receipts. Root selection is exact;
there is no cross-root fallback.

The actor publishes an internal broadcast event only after commit. On the Unix
socket, a newly committed mutating exchange also receives its causally
corresponding `PostCommitEvent` after the durable reply. A replay or refusal
does not emit another event. Revision 1 defines no independent long-lived
subscription operation.

## Wire and authorization

`TranslatorFrame` validates the fixed contract ID and wire revision in the
eight-byte header before body byte-check or deserialization. Wrong contracts,
unsupported revisions, unbound legacy headers, and malformed bodies close the
connection before dispatch.

The request's `AuthorizationClaim` is input, not proof. On every accepted
connection the daemon asks the kernel for the Unix peer UID and derives a
domain-separated `PrincipalId`; request bytes never choose that authenticated
identity. The static deny-by-default policy must then match that principal and
the operation's exact role/capability. The daemon grants the revision-1
authority set only to the explicitly configured UID. Deployments that need
finer separation run clients under distinct service accounts or replace the
policy without weakening peer authentication.

Only `AUTHORITY_ROUTE` is accepted after contract binding. A second daemon
probes an existing socket and refuses to replace a live listener; it removes a
socket only after a refused connection and an unchanged inode check. The
process acquires this socket ownership before opening the embedded database, so
a competing process cannot touch SEMA before discovering the live authority.
The listener owns its exact device/inode pair and removes that node on a
pre-readiness failure or orderly shutdown. Binding refuses a group- or
world-writable runtime directory, so an untrusted UID cannot replace a checked
socket between validation and unlink.

## Donor boundary

The single-owner actor, restart, post-commit notification, and framed-socket
patterns were reseated from the frozen storage implementation. None of its
document records, flat identity authority, universe registry, continuation
intent, allocator scopes, old socket, or central-storage contract exists here.
