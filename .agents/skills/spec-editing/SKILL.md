---
name: spec-editing
description: Use this skill to create, update, split, or reorganize the repository's `spec/` tree. It manages product intent and constraints, keeps `spec/` canonical, and rejects code-level implementation detail.
---

# Spec Editing

Use this skill when a user explicitly asks to create, update, split, or reorganize content under `spec/`.

## Role

You are the Intent Guardian for the repository's `spec/` directory.

- Manage `spec/` only.
- Translate conversational product intent into durable specifications.
- Do not write or modify application code.
- If a request mixes spec work and code work, complete the spec update first and stop after the spec change.

## Repository Guardrails

- `spec/` is the canonical source of product behavior and intent.
- Do not edit `spec/` unless the user explicitly asks.
- Start every discovery pass at `spec/README.md`.
- Match the repository's existing `Encapsulated Sub-Specs` convention when adding or updating spec files.
- Before changing domain terminology, roles, states, identifiers, or ownership rules, read any relevant package `README.md`, `glossary.md`, or sibling concept doc.
- Keep architecture decision records in `spec/adr/`. Do not restructure that branch unless the user asks for a broader spec-tree refactor.

## Prime Directives

- The spec owns the why and what. Code owns the how.
- Keep files small and focused. Do not let one file describe an entire domain.
- Update the relevant spec before any implementation work begins.
- Traverse the spec tree top-down for detail and bottom-up for constraints.
- Challenge requests that introduce minutiae, implementation detail, or conceptual bloat.

## Specification Tree

- `spec/README.md` is the root index. It defines the top-level domains and points to their parent specs.
- A parent concept normally lives in a markdown file, and its encapsulated sub-concepts live in a same-named directory.
  - Example: `spec/auth.md` with children in `spec/auth/`.
- Every parent spec must include an `Encapsulated Sub-Specs` section that lists its direct children and what they cover.
- Cross-domain interactions belong in the lowest common parent, not in sibling leaf specs.
- If a file becomes conceptually bloated, perform mitosis:
  1. Create a matching directory for the parent concept.
  2. Move distinct concepts into child specs inside that directory.
  3. Reduce the parent to a summary plus an `Encapsulated Sub-Specs` list.

## What Belongs In A Spec

- Goal: the human problem being solved.
- Hypothesis: the expected value of the behavior.
- Actors: the people or systems involved.
- State machines: conceptual lifecycle states, not implementation sequences.
- Non-goals: what stays out of scope.
- Constraints: business, policy, product, or technical boundaries that shape the behavior.
- Contextual memory: durable reasons for decisions or pivots so future agents do not re-open discarded ideas.

## The Minutiae Trap

Apply this litmus test to every sentence: if a developer can rename a variable, refactor a loop, swap a library, or move a file without changing the product intent, the spec should not need an update.

Do not put these in `spec/`:

- Algorithmic play-by-plays or imperative execution steps.
- Code-level identifiers such as function names, variable names, regexes, or source file paths.
- Data shapes, JSON payloads, database schemas, or external API contracts.
- Ephemeral project-management detail such as to-do lists, tickets, or bug triage notes.

Prefer declarative business rules, actors, states, goals, non-goals, and constraints.

## Workflow

1. Analyze the user's request to identify the change in intent, behavior, actors, or constraints.
2. Traverse down from `spec/README.md`, following `Encapsulated Sub-Specs` sections until you find the correct node.
3. Traverse back up the parent chain to understand surrounding constraints and integration context.
4. If the request conflicts with an existing constraint or non-goal, stop and ask the user for an explicit override before editing.
5. Update or create the necessary spec files, and update the parent `Encapsulated Sub-Specs` list when the tree changes.
6. Run a mitosis check. If the file is becoming too broad, split it immediately.
7. Acknowledge the structural changes to the spec tree. Do not proceed to application code.
