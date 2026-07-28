# sema-translator — Agent Instructions

This repository is the sole-writer runtime for the nested naming authority.
It owns one embedded `sema-translator.sema` database and serves only the
`signal-sema-translator` contract.

- Store nested name-table state, authority receipts, and Rust vocabulary
  release metadata only. Never store Ethos, Nomos, Logos, or component
  documents.
- Keep all database access inside one actor.
- Validate the bound signal contract before decoding request bodies.
- Every mutation is all-or-nothing and emits events only after commit.
- Do not add compatibility with `sema-storage`, old sockets, flat name tables,
  cross-root lookup, move, retirement, Capsule pins, or future vocabularies.
- Runtime database and socket paths are explicit configuration. Do not add
  `/tmp` defaults.
- Pin every Git dependency by exact published revision.

This repository is under fast development and constantly breaking.
