---
name: goddard-app-sprint-planner
description: Sequence and revise Goddard desktop app implementation sprints using `app/plans/sprints.md`, `app/plans/feature-recommendations.md`, `spec/app.md`, `app/best-practices.md`, and `app/glossary.md`. Use when Codex needs to add a feature to the sprint plan, rebalance sprint boundaries, classify app work as active MVP versus deferred or blocked, or explain the dependency-first rollout of app implementation.
---

# goddard-app-sprint-planner

Maintain the app sprint plan as the dependency-first, MVP-first rollout of the desktop workspace. Keep sprint edits grounded in the existing planning docs instead of inventing a separate roadmap.

## Load The Sequencing Context

- Read `app/plans/sprints.md` first.
- Read `app/plans/feature-recommendations.md`.
- Read `app/best-practices.md` and `app/glossary.md`.
- Read `spec/README.md` and `spec/app.md`.
- Read any feature-specific plan file in `app/plans/` that the sprint change depends on.

## Preserve The Existing Sequencing Rules

- Build shell, repository registry, and tab persistence before feature pages.
- Delay auth and auth-dependent workflows as long as possible.
- Build domain state before connected page components in the same sprint.
- Land cross-domain features only after enough domain data exists to make them useful.
- Keep deferred or spec-blocked work out of the active MVP sequence.

## Place New Work Deliberately

1. Decide whether the feature belongs in an active sprint, `feature-recommendations.md`, or the blocked section.
2. Place the work in the earliest sprint whose prerequisites already exist.
3. Keep local-first features ahead of auth-dependent remote flows when both orders are viable.
4. Group state modules with the components that depend on them.
5. Reuse existing domain groupings before inventing a new sprint.
6. Create a new sprint only when it marks a real dependency boundary, not just to keep lists aesthetically even.
7. Update adjacent sprint goals and rationale when a move changes why the sprint exists.

## Keep Sprint Entries Sharp

- Name goals in user-visible terms, not engineering chores.
- List only the state modules and components needed for that sprint.
- Keep `Why here` explanations dependency-based and brief.
- Call out blocked work explicitly instead of planning around unresolved host or spec constraints.
- Avoid mixing deferred follow-ons into active MVP scope.

## Coordinate With Other Plan Files

- Update `app/plans/feature-recommendations.md` when a feature moves into or out of deferred status.
- Update feature-specific plan docs when sprint placement changes their stated dependencies or rollout assumptions.
- Leave `spec/` unchanged unless a human explicitly asked for spec edits.

## Return A Sequencing Summary

- Explain what moved and why.
- Name the prerequisites that justify the chosen sprint.
- Call out any work that stayed deferred or blocked.
