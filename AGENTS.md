# AI Agent Instructions

## Repository State

- This repository is unreleased and pre-alpha.
- Backwards compatibility is not a requirement.
- Prefer the simplest forward-looking design.
- Do not add legacy compatibility paths, deprecation shims, or fallback behavior unless the user explicitly asks for them.

## App and SDK Parity

- Any new user-facing capability added in the Tauri `app` must also be implemented in `core/sdk`.
- Make both changes in the same PR.
- Do not leave the `app` ahead of `core/sdk` for shipped behavior.
- If a capability cannot yet be supported in `core/sdk`, treat the `app` work as incomplete.

## `spec/` Is Canonical

- The `spec/` folder is the canonical source of product behavior and intent.
- Do not edit the spec unless the user explicitly asks you to.
- Do not knowingly make code, docs, or tests diverge from the spec.
- If a requested change appears inconsistent with the spec, call that out and note that a spec change may be required.

## Concept Docs: `README.md`, `glossary.md`, and Sibling Docs

- This repository keeps domain knowledge close to the code with lightweight concept docs.
- Use them to reduce guessing, avoid re-deriving the same concepts, and keep terminology consistent.

### Document Roles

- `README.md` is the primary home for package boundaries, usage, and integration surface.
- `glossary.md` is the primary home for package-specific domain terms and definitions.
- A sibling concept doc is the primary home for file-local domain concepts beside a dense implementation file, for example `manager.md` next to `manager.ts`.
- Keep canonical definitions in one place. Brief references elsewhere are fine, but avoid copy-pasted redefinitions across layers.

### When to Read Them

- Read a package `glossary.md` before changing domain behavior, naming, states, roles, identifiers, or ownership rules in a package that has one.
- Read a sibling concept doc before editing its adjacent implementation file or another file that depends on the same local model.
- Treat relevant concept docs as required context when they exist, not optional background reading.

### When to Create or Expand Them

- Add or expand a package `glossary.md` when a package has recurring project-specific terms used across multiple files.
- Add or expand a sibling concept doc when one file contains dense local abstractions that are slow to recover from code comments, types, or signatures alone.
- Strong signals include:
  - lifecycle states or transitions
  - role or permission systems
  - ownership boundaries
  - queue, ledger, projection, or retry models
  - multiple closely related identifiers
  - invariants or allowed state transitions that are easy to misread from implementation alone
- If a glossary or sibling concept doc is clearly warranted and missing, create it in the same PR.
- If a file is split or responsibilities move, reassess whether the concepts still belong in a sibling doc or should move into a package glossary.

### When to Update Them

- Update the relevant concept doc in the same PR when you add, remove, rename, or change the meaning of a domain concept.
- Treat changes to terminology, invariants, lifecycle rules, ownership rules, or allowed transitions as concept changes.
- If the domain model is unchanged and only implementation details moved, do not churn these docs.
- When code comments and concept docs disagree, make them consistent in the same change.
- Before introducing new types, statuses, events, commands, or identifiers, check for established terminology in the relevant concept docs.
- If the right term is missing, add it in the same change instead of leaving it implicit.

### Format and Content Rules

- Use one `#` H1 at the top.
- Prefer bullet lists and keep prose minimal.
- Use nested bullets when extra structure or nuance helps.
- Keep these docs concise.
- Focus on the domain-level what and why.
- Do not turn them into implementation walkthroughs, code tours, API references, change logs, or behavior logs.
- Do not document incidental mechanics that are already obvious from the code.

## Code Style Guidelines

- Inline values instead of introducing single-use variables unless the variable materially improves readability or avoids repeating a complex expression.
- Every exported module declaration must have a human-readable `/** ... */` description comment.
- Every TypeScript type alias and interface, whether exported or internal, must have a human-readable `/** ... */` description comment.
- Every non-trivial top-level function must have a human-readable `/** ... */` description comment.
- These comments should explain the non-obvious what and, when useful, the why.
- Strange coding patterns must have a brief `//` comment stating what they are doing and why they are necessary.
- Do not add JSDoc tag boilerplate such as `@param` or `@returns`.
- Do not document the obvious or describe line-by-line implementation mechanics.
- If a non-trivial top-level function truly has nothing useful to document, you may omit the block and optionally leave a short `//` comment explaining the absence.

## Git / Rebase Note

- In non-interactive terminals, use `GIT_EDITOR=true git rebase --continue` to avoid hanging in an interactive editor.

## Commit Message Format

- Follow Conventional Commits.
- Required format: `<type>(optional-scope): <description>`
- Common types include: `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `ci`, `build`, `perf`, `revert`
- For breaking changes, use `!` after the type or scope, or include a `BREAKING CHANGE:` footer.
- Keep the subject concise and imperative.
- Prefer atomic commits with one clear purpose.
- Split policy or docs-only changes from behavior or test changes unless they are inseparable.
- Every commit must include a body.
- The body must use clear bullet points and explain what changed and why, not just restate the subject.

## Testing Policy

- Do not add automated tests for the `app/` package.
- Keep the rest of the test suite lean and intentional.
- Do not keep a test just because it already exists.
- Remove tests that do not protect a meaningful contract, user-visible behavior, regression boundary, or shared mock drift check.
- Prefer contract-level tests over implementation-detail tests.
- In Vitest files, use `expect` rather than `assert` from either Vitest or Node.
- Avoid assertions against incidental fragments, informal constants, or wording-heavy output when a shorter contract-level check will cover the behavior.
- Exact checks for real contract strings, stable literals, or other intentionally durable outputs are acceptable.
- Prefer stable signals such as commands, structured fields, intent markers, ownership data, and explicit contract strings over long natural-language prose.
- When replacing a shallow or brittle test, remove the old test unless it still protects distinct behavior.
