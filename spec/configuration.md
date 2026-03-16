# Configuration

## Goal
Define a clear, user-facing configuration hierarchy for Goddard so individuals, repositories, and runtime callers can shape behavior without ambiguity about scope or precedence.

## Hypothesis
We believe that separating global defaults, repository overrides, and runtime overrides will make Goddard easier to adopt across multiple repositories while preserving precise control for one-off execution.

## Primary Actors
- Developer maintaining personal defaults across repositories
- Repository maintainer defining shared repository behavior
- SDK consumer supplying execution-time overrides
- Runtime host resolving configuration before work begins

## Configuration Hierarchy

### Global Configuration
- Provides the default configuration baseline for a user across repositories.
- Lives under `~/.goddard/`.
- Uses `~/.goddard/config.ts` as the user-level default entrypoint.
- Establishes reusable defaults that should apply unless narrowed by repository or runtime intent.

### Local Configuration
- Overrides global configuration for a specific repository.
- Lives under `${repositoryRoot}/.goddard/`.
- Uses `${repositoryRoot}/.goddard/config.ts` as the repository-level default entrypoint.
- Captures shared team or project intent without requiring each developer to duplicate the same settings globally.

### Runtime Configuration
- Overrides local configuration for a specific invocation.
- Is supplied directly to SDK functions at execution time.
- Exists to support contextual, temporary, or host-specific adjustments without mutating persisted defaults.

## Resolution Model
- Precedence is deterministic: global configuration is the baseline, local configuration overrides global, and runtime configuration overrides local.
- Persisted configuration defines durable defaults; runtime configuration defines the final invocation-specific intent.
- Configuration should resolve before execution begins so the active runtime operates against a stable view of intent.

## Configurable Entities

### Loops
- Loops are named automation definitions that can be resolved from configuration roots and started at runtime.
- A loop may be defined as `.goddard/loops/<name>.md` or as a directory-based loop package at `.goddard/loops/<name>/prompt.{md,ts}`.
- Markdown-based loop definitions may include YAML frontmatter for loop-specific configuration.
- Directory-based loop packages may include a `config.json` file beside `prompt.{md,ts}` to express package-scoped defaults.
- Loops are loaded by name at runtime.
- Runtime hosts may also supply in-memory, throwaway loop configurations for execution that does not need a persisted on-disk definition.

### Actions
- Actions are named, reusable one-shot execution definitions that can be resolved from configuration roots and invoked at runtime.
- An action may be defined as `.goddard/actions/<name>.md` or as a directory-based action package at `.goddard/actions/<name>/prompt.{md,ts}`.
- Markdown-based action definitions may include YAML frontmatter for action-specific configuration.
- Directory-based action packages may include a `config.json` file beside `prompt.{md,ts}` to express package-scoped defaults.
- Actions are loaded by name at runtime.

## Constraints
- Repository-scoped configuration must be able to override user-scoped defaults without mutating the user-level source of truth.
- Runtime overrides must remain ephemeral and must not implicitly rewrite persisted configuration.
- Named entities should be discoverable through the same configuration roots used for baseline defaults so repository intent stays co-located.
- The configuration model must support both human-authored markdown definitions and richer package-style definitions.
- Configuration behavior must remain aligned across SDK consumers and the desktop app; the app must not invent a parallel configuration model.

## Non-Goals
- Defining the exact data shape for configuration payloads.
- Specifying parser internals, merge algorithms, or validation library choices.
- Documenting every possible configurable field for loops, actions, or runtime hosts in this file.
- Treating temporary runtime overrides as durable repository or user preferences.

## Decision Memory
- Goddard needs a configuration model that works at three levels of intent: personal defaults, repository defaults, and invocation-time overrides.
- Named loops and actions are part of the product surface, not ad hoc files, so their discovery locations and packaging modes should be explicit at the specification level.
- Repository-local configuration is the collaboration boundary; runtime input is the experimentation boundary.
