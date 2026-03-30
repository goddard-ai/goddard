---
name: goddard-app-feature-planner
description: Plan, revise, and review Goddard desktop app features using `app/plans/`, `spec/app.md`, `app/best-practices.md`, and `app/glossary.md`. Use when Codex needs to draft a new app feature plan, update or split an existing plan, reconcile a proposed feature with current app constraints, or identify the state modules, components, dependencies, and deferred follow-ons for an app change.
---

# goddard-app-feature-planner

Plan app features as doc changes grounded in the current spec and app architecture. Keep plans tab-first, local-first, thin over shared contracts, and explicit about what is MVP, deferred, or blocked.

## Load The Planning Context

- Read `app/README.md`, `app/best-practices.md`, and `app/glossary.md`.
- Read `spec/README.md` and `spec/app.md`.
- Read `app/plans/sprints.md` and `app/plans/feature-recommendations.md`.
- Read any nearby plan file in `app/plans/` that overlaps the requested feature.
- Read the relevant upstream package or platform docs only when the plan depends on specific third-party API or platform constraints.

## Plan One Feature At A Time

1. Name the feature with existing app nouns from the glossary.
2. Decide whether the request belongs in a dedicated plan file under `app/plans/`, in `app/plans/feature-recommendations.md`, or in a spec discussion because it conflicts with the current product or host model.
3. State the user goal and why the feature matters to the desktop workspace.
4. Identify the minimum state modules, pages, dialogs, and detail tabs required.
5. Identify shared daemon, SDK, or schema dependencies instead of inventing app-only payloads.
6. Separate MVP scope from follow-on enhancements.
7. Record explicit blockers or open questions instead of silently planning around spec gaps.
8. Mention likely sprint fit only as a short note unless the user explicitly asked to resequence sprints.

## Respect These App Constraints

- Keep the app tab-first and workspace-first.
- Keep shared state out of components and favor state modules with semantic actions.
- Keep async work out of presentational components.
- Keep host effects behind Electrobun RPC adapters.
- Keep the app thin over shared SDK, daemon, and schema contracts.
- Keep auth lazy and gate only protected actions.
- Keep list-first pages for operator workflows unless the product direction changes.
- Keep terminal, preview, and other host-boundary-heavy ideas clearly marked as blocked when the architecture is unresolved.

## Write Concise Plan Docs

- Use short sections such as `Goal`, `Scope`, `State`, `Components`, `Dependencies`, `Open Questions`, and `Why now` only when they help.
- Prefer stable nouns like `RepositoriesPage`, `SessionLaunchState`, or `PullRequestView` instead of generic module names.
- Describe behavior and UI ownership, not implementation walkthroughs or JSX details.
- Avoid code-level TODO dumps and backlog sludge.

## Route Related Edits Correctly

- Update `app/plans/feature-recommendations.md` when the result is intentionally deferred follow-up work.
- Update `app/plans/sprints.md` only when the user explicitly asks for sprint sequencing or when the feature plan cannot stay correct without a matching sprint change.
- Update `spec/` only when a human explicitly asks. Otherwise call out mismatches instead of editing around them.
