# Sema Translator

This repository owns the authority-approved bootstrap translation boundary.

- Accept only explicit, already-minted identities and exact authority revisions.
- Bind approvals to the complete prepared draft; refuse missing, unused, stale,
  or configuration-mismatched seats.
- Keep identity spelling separate from identity authority. Never mint, infer, or
  content-address an identity from source text.
- Do not add an engine, actor, database, store, daemon, socket, wire protocol,
  runtime feature, or deployment surface. Runtime execution belongs to
  `sema-engine`.
- Pin every Git dependency to one exact published revision.

This repository is under fast development and constantly breaking.
