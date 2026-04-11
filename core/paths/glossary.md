# Paths Glossary

- Scope:
  - This glossary covers path-only concepts owned by `@goddard-ai/paths`.
- `Goddard Global Directory`
  - The user-scoped `~/.goddard` root.
  - Why: so daemon and SDK features resolve one shared global home without duplicating path math.
- `Goddard Local Directory`
  - The repository-scoped `.goddard` root under one working directory.
  - Why: so config and package discovery can resolve local Goddard assets consistently.
- `Derived Path`
  - A deterministic child path under a global or local `.goddard` root.
  - Why: so path construction stays pure while persistence ownership stays in higher-level packages.
- `Goddard Cache Directory`
  - The user-scoped OS cache root used for disposable Goddard runtime data.
  - Why: so refreshable runtime artifacts such as cloned registries can live outside the durable `.goddard` home.
- `Host Path Surface`
  - One runtime-specific entry point that resolves the same Goddard path concepts for a specific host.
  - Why: so Node and Tauri can share filenames and directory names without sharing incompatible runtime APIs.
