# Sema Translator

This repository owns the authority-approved bootstrap translation boundary.

- Accept only source text and source placement from callers. The Sema
  authority mints all `EncodedName` values privately with a CSPRNG.
- Keep `TextualName` metadata outside strict identities. The authority alone
  stages the exact metadata transition, receipt, and direct `TrueNamed`
  declaration values; callers cannot construct those capabilities.
- Refuse bundled Stream declarations before allocation or staging. Do not add
  a compatibility adapter for the retired vocabulary/seat contract.
- Do not add an engine, actor, database, store, daemon, socket, wire protocol,
  runtime feature, or deployment surface. Runtime execution belongs to
  `sema-engine`.
- Pin every Git dependency to one exact published revision.

This repository is under fast development and constantly breaking.
