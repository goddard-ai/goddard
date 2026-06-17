<critical-rules>
- Do not overwrite or revert unrelated user changes.
- Do not edit `spec/` unless explicitly asked.
- Do not knowingly let code, non-public docs, or tests diverge from `spec/`; public `docs/` describe current supported behavior and may diverge when `spec/` describes an ideal or future shape.
- This repo is unreleased and pre-alpha; do not add legacy compatibility paths, deprecation shims, or fallback behavior unless explicitly asked.
- Do not use destructive git commands unless explicitly requested.
- Commit completed file changes with Conventional Commits before ending the turn unless the user or a safer scoped workflow says not to.
- The pre-commit hook automatically formats staged changes; do not run the repo `fmt` command unless explicitly asked.
- Run or attempt the required verification before finishing, and report any limitation.
</critical-rules>

<rulesets>
Rules live in `.agents/rules/`. Read every matching ruleset before acting:

- `product-contracts.md`: MUST read when touching product behavior, `spec/`, SDK/app parity, shared data loading or mutation, system configuration, or user-visible capabilities.
- `implementation.md`: MUST read when editing production code, refactoring, changing architecture, adding abstractions or exports, or changing dependencies.
- `defaults.md`: MUST read when adding or changing config, normalization, persisted data shapes, SDK inputs, domain defaults, or resolved values.
- `features.md`: MUST read when adding or changing cross-layer feature packages, feature schemas, daemon plugins, SDK/app feature wiring, or scaffolded features.
- `testing.md`: MUST read when adding, changing, reviewing, or deciding whether to add tests, and when verifying behavior changes.
- `documentation.md`: MUST read when changing terminology, concepts, package boundaries, README or glossary docs, user-visible features, or undocumented feature tracking.
- `overview.md`: MUST read when creating, reorganizing, or editing an `overview/` documentation folder.
- `git.md`: MUST read before staging, committing, reviewing diffs, splitting work, or finishing any file-changing task.
- `app.md`: MUST read before editing anything under `app/`; it routes to additional app-specific rulesets and skills.
</rulesets>
