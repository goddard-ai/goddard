# Configuration

## Goal
Define a clear, user-facing configuration hierarchy for Goddard so individuals, repositories, named automation entities, and runtime callers can shape behavior without ambiguity about scope or precedence.

## Hypothesis
We believe that separating personal defaults, repository defaults, entity-level overrides, and runtime overrides will make Goddard easier to adopt across multiple repositories while preserving precise control for reusable automation and one-off execution.

## Primary Actors
- Developer maintaining personal defaults across repositories
- Repository maintainer defining shared repository behavior
- Author or maintainer of a Loop or Action definition
- SDK consumer supplying execution-time overrides
- Runtime host resolving configuration before work begins

## Configuration Hierarchy

### 1. Global Defaults
- Provides the default configuration baseline for a user across repositories.
- Lives under `~/.goddard/`.
- Uses `~/.goddard/config.ts` as the user-level default entrypoint.
- Establishes reusable defaults that should apply unless narrowed by repository, entity, or runtime intent.

### 2. Local Overrides
- Overrides global defaults for a specific repository.
- Lives under `${repositoryRoot}/.goddard/`.
- Uses `${repositoryRoot}/.goddard/config.ts` as the repository-level default entrypoint.
- Captures shared team or project intent without requiring each developer to duplicate the same settings globally.

### 3. Entity Configuration
- Overrides the inherited global and local defaults for a specific Loop or Action.
- Is defined within the Loop or Action itself, such as through markdown frontmatter or package-scoped configuration files.
- Exists so a reusable automation entity can carry its own intent without redefining the broader repository or user baseline.

### 4. Runtime Configuration
- Overrides global, local, and entity configuration for a specific invocation.
- Is supplied directly to SDK functions at execution time.
- Exists to support contextual, temporary, or host-specific adjustments without mutating persisted defaults.

## Resolution Model
- Precedence is deterministic: global configuration is the baseline, local configuration overrides global, entity configuration overrides the inherited global and local defaults for a specific Loop or Action, and runtime configuration is the final override.
- Loops and Actions inherit their default configuration from the same global and local configuration modules used for broader Goddard behavior, then apply any entity-specific overrides before execution begins.
- Persisted configuration defines durable defaults at the user, repository, and entity levels; runtime configuration defines the final invocation-specific intent.
- Configuration should resolve before execution begins so the active runtime operates against a stable view of intent.

## Configurable Entities

### Loops
- Loops are named automation definitions that can be resolved from configuration roots and started at runtime.
- A loop may be defined as `.goddard/loops/<name>.md` or as a directory-based loop package at `.goddard/loops/<name>/prompt.{md,ts}`.
- Loop definitions inherit the resolved global and local defaults from `.goddard/config.ts` before applying any loop-specific configuration.
- Markdown-based loop definitions may include YAML frontmatter for loop-specific configuration.
- Directory-based loop packages may include a `config.json` file beside `prompt.{md,ts}` to express package-scoped defaults.
- Runtime hosts may also supply in-memory, throwaway loop configurations for execution that does not need a persisted on-disk definition.

### Actions
- Actions are named, reusable one-shot execution definitions that can be resolved from configuration roots and invoked at runtime.
- An action may be defined as `.goddard/actions/<name>.md` or as a directory-based action package at `.goddard/actions/<name>/prompt.{md,ts}`.
- Action definitions inherit the resolved global and local defaults from `.goddard/config.ts` before applying any action-specific configuration.
- Markdown-based action definitions may include YAML frontmatter for action-specific configuration.
- Directory-based action packages may include a `config.json` file beside `prompt.{md,ts}` to express package-scoped defaults.
- Actions are loaded by name at runtime.

## Constraints
- Repository-scoped configuration must be able to override user-scoped defaults without mutating the user-level source of truth.
- Loop and Action definitions must inherit the shared user and repository baseline before applying their own overrides.
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
- Goddard needs a configuration model that works at four levels of intent: personal defaults, repository defaults, reusable entity defaults, and invocation-time overrides.
- Named loops and actions are part of the product surface, not ad hoc files, so their discovery locations and packaging modes should be explicit at the specification level.
- Repository-local configuration is the collaboration boundary, entity configuration is the reusable automation boundary, and runtime input is the experimentation boundary.
