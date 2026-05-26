---
name: spec-editing
description: Update the repository `spec/` tree as capability contracts that describe what Goddard must do, what boundaries it must respect, and how specs are organized.
---

# Spec Editing

Use this skill when the user explicitly asks to create, update, split, or reorganize content under `spec/`.

## Purpose

Spec files are capability contracts. They describe the product behavior Goddard must eventually provide and the boundaries that behavior must obey.

They are not proposals, implementation plans, status reports, issue trackers, API references, or persuasive product briefs.

## Scope

- Work in `spec/` only.
- Do not edit `spec/` unless the user explicitly asks.
- If a request mixes spec work and code work, complete the spec update first and stop after the spec change.

## Tree Shape

- Start from `spec/README.md`.
- `spec/README.md` is the root index for top-level spec areas.
- A parent concept may live in `spec/name.md`, with children in `spec/name/`.
- Parent specs with children include an `Encapsulated Sub-Specs` section listing direct children.
- Architecture decision records live in `spec/adr/`; do not restructure that branch unless asked.
- When one file stops describing one coherent product surface, split it and leave the parent as a concise map.

## Writing Values

- Prefer direct statements of required capability: what the system supports, prevents, preserves, or guarantees.
- Capture constraints and boundaries when they shape product behavior.
- Keep only information that should remain true after the implementation is rewritten.
- Choose headings that fit the content; do not impose default sections.
- Add rationale or history only when it prevents a likely future mistake.
- Keep the smallest wording that preserves the intended product behavior.

## Keep Out

- Implementation plans, algorithms, parser mechanics, storage mechanics, or execution play-by-plays.
- Code-level identifiers, source file paths, JSON payloads, database schemas, or external API minutiae.
- Backlog items, ticket notes, temporary status, bug triage, or release planning.
- Sections added only to satisfy a template.
- Rationale that merely sells the feature instead of constraining future decisions.

## Workflow

1. Read from `spec/README.md` to find the narrowest relevant spec node.
2. Edit the smallest set of spec files needed to express the capability or constraint.
3. Update `Encapsulated Sub-Specs` only when the tree changes.
4. Re-read the changed text and remove template prose, implementation detail, and repeated motivation.
