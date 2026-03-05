# AGENTS.md — Operating Guide for `goddard-ai/roadmap`

## Repository role

This repository is a **planning and proposal workspace** for Goddard.

It is not the implementation repo. The primary job here is to:
- capture product intent,
- draft and refine initiative proposals,
- maintain proposal quality and planning clarity,
- keep planning artifacts clear, reviewable, and execution-ready.

In practice, **most agent work in this repo should be proposal authoring/editing**.

---

## Source of truth and boundaries

### Core planning files (editable)
- `spec/vision.md` — product direction and principles (synced reference)
- `proposals/` — approved initiative-level proposals ready for implementation planning. Unapproved proposals exist as pull requests. Implemented proposals are removed.
- `README.md` — repo overview and navigation

### Synced spec mirror (generally not edited directly)
- `spec/` — synced from `goddard-ai/goddard` (`spec-only` flow)

Treat `spec/` as a **reference mirror** unless the task explicitly asks to modify subrepo wiring/sync metadata.

### Accessing source code
If you need to access the Goddard implementation source code for context, you may clone the repository. It is recommended to perform a shallow clone into a local `src/` directory (which is gitignored):
```bash
git clone --depth 1 https://github.com/goddard-ai/goddard.git src/
```

---

## Primary agent workflow (proposal-first)

When asked to add or update ideas:

1. **Read context first**
   - `spec/vision.md`
   - any existing related proposal(s)
   - any project-specific guidelines in `AGENTS.md`

2. **Create/update proposal PRs or files in `proposals/`**
   - New proposals are drafted and reviewed via pull requests. Do not commit unapproved proposals to the main branch.
   - Once a proposal is approved, it is merged into the `proposals/` directory.
   - Once a proposal is fully implemented, it should be deleted from this repository.
   - One file per initiative
   - Use filenames in the format `YYYYMMDDHH_kebab-case-title.md`
   - Keep scope focused and review-friendly

3. **Use this standard proposal structure**
   1. Problem statement
   2. Proposed scope
   3. Constraints and non-goals
   4. Success metrics
   5. Review/approval status

4. **Cross-check consistency**
   - Ensure proposal names/terms align with `README.md` and `spec/vision.md`

5. **Prefer clarity over implementation detail**
   - Focus on **what/why**, not low-level how
   - If implementation specifics are requested, frame them as assumptions or open questions unless explicitly required

---

## Decision and writing guidelines

- Keep docs concise, scannable, and actionable.
- Preserve human approval gates for AI-generated work.
- Explicitly call out risks, dependencies, and open questions.
- Avoid speculative technical architecture unless needed for scope clarity.
- Do not introduce autonomous behavior that bypasses review/approval.

---

## Git and change hygiene

- Make atomic doc changes with clear commit messages (`docs:` or `chore:` prefixes).
- Do not rewrite unrelated files.
- If subrepo metadata (`spec/.gitrepo`) must change, document why in the commit message.

---

## Out-of-scope in this repo

- Shipping application code/features
- Deep technical implementation plans tied to runtime internals
- Replacing product management tooling behavior (e.g., ClickUp) rather than defining integrations

If a request is implementation-heavy, produce a proposal-ready artifact here and note the handoff to the implementation repository.
