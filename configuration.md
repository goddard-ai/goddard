# Configuration

## Goal
Define a clear configuration model for Goddard in which persisted settings are always machine-readable and machine-writable, so humans and tools can safely inspect, generate, and update behavior across user, repository, entity, and runtime scopes.

## Hypothesis
We believe that separating user defaults, repository defaults, entity-level defaults, and runtime overrides into a deterministic JSON-based model will make Goddard easier to automate, easier to maintain across repositories, and less ambiguous for both humans and machines.

## Primary Actors
- Developer maintaining personal defaults across repositories
- Repository maintainer defining shared repository behavior
- Author or maintainer of a Loop or Action definition
- SDK consumer supplying execution-time overrides
- Runtime host resolving configuration before work begins
- Automated tools that need to read or update persisted configuration safely

## Configuration Hierarchy

### 1. Global Defaults
- Provides the baseline configuration for an individual across repositories.
- Is persisted as a user-scoped JSON document.
- Establishes reusable defaults that should apply unless narrowed by repository, entity, or runtime intent.

### 2. Local Overrides
- Overrides global defaults for a specific repository.
- Is persisted as a repository-scoped JSON document.
- Captures shared team or project intent without requiring each developer to duplicate the same settings globally.

### 3. Entity Configuration
- Overrides inherited global and local defaults for a specific Loop or Action.
- Is persisted as machine-readable JSON associated with that named entity.
- Exists so a reusable automation entity can carry its own defaults without redefining the broader repository or user baseline.

### 4. Runtime Configuration
- Overrides global, local, and entity configuration for a specific invocation.
- Is supplied directly by the calling runtime and remains ephemeral.
- Exists to support contextual, temporary, or host-specific adjustments without mutating persisted defaults.

## Resolution Model
- Precedence is deterministic: global configuration is the baseline, local configuration overrides global, entity configuration overrides the inherited global and local defaults for a specific Loop or Action, and runtime configuration is the final override.
- Persisted configuration resolves from user, repository, and entity JSON sources before any runtime override is applied.
- Loops and Actions inherit the same user and repository baseline used for broader Goddard behavior, then apply any entity-specific defaults before execution begins.
- Configuration should resolve before execution begins so the active runtime operates against a stable view of intent.

## Configurable Entities

### Loops
- Loops are named automation definitions that can be resolved from configuration roots and started at runtime.
- A loop may be represented as a prompt document or as a richer packaged definition.
- If a loop carries persisted defaults, those defaults must live in machine-readable JSON associated with the loop rather than inside the prompt content itself.
- Runtime hosts may also supply in-memory, throwaway loop configuration for execution that does not need a persisted on-disk definition.

### Actions
- Actions are named, reusable one-shot execution definitions that can be resolved from configuration roots and invoked at runtime.
- An action may be represented as a prompt document or as a richer packaged definition.
- If an action carries persisted defaults, those defaults must live in machine-readable JSON associated with the action rather than inside the prompt content itself.
- Actions are loaded by name at runtime.

## Constraints
- All persisted configuration must be machine-readable and machine-writable.
- Persisted configuration must be stored as JSON rather than executable source.
- Repository-scoped configuration must be able to override user-scoped defaults without mutating the user-level source of truth.
- Loop and Action definitions must inherit the shared user and repository baseline before applying their own overrides.
- Prompt content must not double as a configuration transport; document metadata is not a supported configuration surface.
- Runtime overrides must remain ephemeral and must not implicitly rewrite persisted configuration.
- Named entities should be discoverable through the same configuration roots used for baseline defaults so repository intent stays co-located.
- Configuration behavior must remain aligned across SDK consumers and the desktop app; the app must not invent a parallel configuration model.

## Non-Goals
- Defining the exact data shape for configuration payloads.
- Specifying parser internals, merge algorithms, or validation library choices.
- Documenting every possible configurable field for loops, actions, or runtime hosts in this file.
- Treating temporary runtime overrides as durable repository or user preferences.
- Using executable modules or document headers as alternative persisted configuration mechanisms.

## Decision Memory
- Goddard needs a configuration model that works at four levels of intent: personal defaults, repository defaults, reusable entity defaults, and invocation-time overrides.
- Persisted JSON keeps configuration accessible to both humans and automation without requiring code execution to inspect or update it.
- Separating prompt content from configuration avoids ambiguous editing surfaces and makes automation safer.
- Repository-local configuration is the collaboration boundary, entity configuration is the reusable automation boundary, and runtime input is the experimentation boundary.
