---
name: goddard-contributor
description: Repository contribution guide for the Goddard workspace. Use when working in this repository and you need long-form guidance that does not belong in AGENTS.md, including repository contribution policy, documentation policy, testing policy, or app implementation patterns migrated from CONTRIBUTING.md and best-practices.md files.
---

# Goddard Contributor

Use this skill after reading the applicable `AGENTS.md` files. It is the repository's source for long-form contribution guidance that should not live in `AGENTS.md`.

## Workflow

1. Read the root and nearest local `AGENTS.md` files first.
2. Load only the references that match the current task. Do not read every reference by default.
3. Treat `AGENTS.md` as the bootstrap source for hard rules. Use this skill for longer policy, subtree-specific implementation guidance, and terminology.
4. When you change a concept or policy covered by one of these references, update the matching reference file and keep the original doc-path signpost aligned.

## Reference Map

- Repo-wide documentation, dependency, expanded code-style, and testing policy: `references/repository-contributing.md`
- App-local contribution rules that do not belong in `app/AGENTS.md`: `references/app-contributing.md`
- Detailed app implementation patterns: `references/app-best-practices.md`
- App form state, async field loading, and dialog-form composition patterns: `references/app-form-patterns.md`

## Loading Hints

- When editing `app/`, usually read `references/app-contributing.md` and `references/app-best-practices.md`.
- When building or refactoring app forms, also read `references/app-form-patterns.md`.
- When changing tests or deciding whether to add tests, read `references/repository-contributing.md` before editing code.
- When changing tests or deciding whether to add tests in `app/`, also read `references/app-contributing.md`.
- Testing default: keep tests for meaningful observable behavior or fragile contracts; prefer a brief code comment over a new test when the goal is just to preserve implementation-detail context, a platform workaround, or a Chesterton's Fence-style invariant.
- When changing naming, states, roles, identifiers, or ownership rules, read the relevant `glossary.md` before editing code.
