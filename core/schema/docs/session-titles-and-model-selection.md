# Session Titles and Title Generator Model Selection

## Status

Proposed

## Overview / Problem Statement

Goddard needs a short human-readable title for every daemon session so session rows, chat headers, and related surfaces can identify work without exposing raw ids or long prompt snippets.

The title should exist immediately when a session is created, even when no external model is configured. When a title-generation model is configured, Goddard should refine that fallback title asynchronously without blocking session creation.

This feature is separate from the session agent's own model selection:

- `session.model` continues to mean the default agent model for new sessions.
- title generation needs its own configuration surface.
- title generation must not create or reuse agent sessions.

## Context

- `app/plans/components/SessionListRow.md` and `app/plans/components/SessionChatHeader.md` already assume that a session summary exposes `title`.
- `spec/configuration.md` requires shared JSON-backed configuration and rejects app-only parallel config models.
- The feature needs provider-backed text generation only for the short title-refinement step, not for the primary daemon session lifecycle.
- The repository is pre-alpha, so the simplest forward-looking design is preferred.

## Goals

- Return a non-empty durable session title from the initial `sessionCreate` response.
- Show a deterministic fallback title immediately.
- Allow users to configure a preferred provider and model for title generation independently from the session agent model.
- Generate a better title asynchronously when title-generation config is present and usable.
- Support many providers through one small abstraction boundary instead of provider-specific session logic.
- Build the persisted provider/model config surface on top of `ai-sdk-json-schema` instead of inventing a parallel schema.
- Keep JSON-to-runtime AI model resolution in a reusable module so future config-backed AI features can share it.
- Keep title generation failure non-fatal to session creation and normal prompting.

## Non-Goals

- Changing how the session agent's own model is selected.
- Reusing `session.model` for title generation.
- Creating ACP agent sessions just to generate titles.
- Tying title generation to any live session control path.
- Manual session rename or repeated re-titling over the life of a session.
- Persisting raw provider API tokens in Goddard config.
- A new realtime stream dedicated only to title updates.

## Success Criteria

- A fresh session list row never renders a blank title.
- When no title-generation provider is configured, Goddard still produces a stable fallback title and does not attempt remote generation.
- When title-generation config is present and valid, Goddard refines the fallback title in the background and updates the persisted session record.
- When title-generation config is missing, invalid at runtime, or lacks credentials, the user still gets the fallback title and the session remains usable.
- The design introduces no extra daemon session records and no extra ACP session traffic for title generation.

## Assumptions and Constraints

- Title generation is an auxiliary subsystem owned by the daemon, not by the session manager's ACP runtime.
- The daemon should use a direct provider abstraction for title generation. This is the correct seam for a dependency like Vercel `ai`.
- `ai-sdk-json-schema@0.3.0` is available and should be treated as the source of truth for persisted text-model config validation and runtime loading helpers.
- Provider credentials should be resolved from environment variables, not persisted token values.
- The feature should work in degraded local-only mode by falling back to deterministic prompt truncation.
- Local root config may override global root config for title generation, but this feature does not need action-level, loop-level, or per-session overrides in v1.

## Terminology

- Fallback Title
  - A deterministic short title derived locally from the first user prompt text.
  - It is always available without external model access.
- Generated Title
  - A short refined title produced asynchronously by the daemon's title-generation subsystem.
- Text Model Config
  - The persisted `{ provider, model }` JSON shape validated by `ai-sdk-json-schema`.
- Title Generator Config
  - The persisted shared configuration that selects one reusable text model config for title generation.

## Proposed Design

### 1. Make session title a first-class session field

Add the following fields to the durable daemon session contract:

- `title: string`
- `titleState: "placeholder" | "fallback" | "pending" | "generated" | "failed"`

Field semantics:

- `placeholder`
  - No user prompt text has been seen yet, so the title is the generic placeholder `New session`.
- `fallback`
  - A deterministic prompt-derived title is present and no background generation is running.
- `pending`
  - The fallback title is visible and background generation is in progress.
- `generated`
  - The current title is a validated provider-generated title.
- `failed`
  - The fallback title remains visible after a generation attempt failed.

This gives hosts enough information to render the visible title correctly without inventing app-local heuristics.

### 2. Add a new dedicated title-generator config surface

Do not reuse `session.model`.

Introduce a new root-level shared config section built on `ai-sdk-json-schema`'s text model config contract:

```ts
import type { ModelConfig } from "ai-sdk-json-schema"

type UserConfig = {
  sessionTitles?: {
    generator?: ModelConfig
  }
}
```

Semantics:

- `generator`
  - Stores a persisted `{ provider, model }` pair for the title generator.
- `provider`
  - Selects one provider id known to `ai-sdk-json-schema`.
- `model`
  - Selects the model id for that provider.

Validation and JSON Schema generation should come from the package exports:

- `textModelConfigSchema` for runtime validation
- `textModelConfigJsonSchema` for generated config-schema composition

When generating Goddard's own `goddard.json` JSON Schema:

- import `textModelConfigJsonSchema` from the installed `ai-sdk-json-schema` package at generation time;
- embed that schema once inside the generated `goddard.json` output under one local `$defs` entry;
- point `sessionTitles.generator` at that embedded definition with a local `$ref`;
- do not reference a GitHub URL, package URL, or any other remote schema location;
- do not duplicate the full text-model schema inline at each field site, because it is large and should exist only once per generated schema artifact.

Resolution precedence for this new config is:

1. Local root `sessionTitles.generator`
2. Global root `sessionTitles.generator`

This feature does not use action config, loop config, or per-session runtime overrides in v1 because title generation is an operator preference, not task semantics.

### 3. Keep provider resolution in a reusable daemon module

Introduce a daemon-owned module whose only responsibility is to map JSON text-model config values into runtime AI SDK model instances.

Recommended boundary:

- one input: validated shared JSON config;
- one output: a ready-to-use runtime model instance plus any normalized resolver metadata;
- no title-specific prompt logic;
- no session-manager coupling;
- no persistence side effects.

Recommended implementation location:

- a daemon-local module such as `core/daemon/src/ai/text-model-resolver.ts` or equivalent, owned by the provider-integration layer rather than by session-title code.

This module should be reusable by future config-backed AI features, not owned by the title generator itself.

Implementation basis:

- validate with `textModelConfigSchema`;
- inspect metadata with `resolveModel("text", ...)` when helpful for diagnostics;
- load runtime models with `loadTextModel` for the simple path;
- keep `buildModelLoadPlan("text", ...)` available if Goddard later needs explicit package-audit or module-load control.

Example responsibility split:

- provider resolver module
  - understands persisted text-model config through `ai-sdk-json-schema`;
  - reads environment variables;
  - delegates model loading to `loadTextModel` or an equivalent load-plan path;
  - reports unknown-provider, unknown-model, missing-package, or missing-credential failures in one normalized shape.
- title generation module
  - reads `sessionTitles.generator`;
  - calls the provider resolver module;
  - performs generation and title validation.

### 4. Use a daemon-internal provider utility, not an agent session

Title generation should run through a dedicated daemon utility based on Vercel `ai`:

- use `ai` for the generation call and normalized text result handling;
- resolve the configured text model through the reusable daemon resolver module;
- keep this provider logic out of `SessionManager` ACP transport code;
- do not create any daemon session record, ACP connection, or agent subprocess for title generation.

This is the right dependency tradeoff:

- the feature genuinely needs cross-provider text generation;
- it is isolated to one auxiliary path;
- it avoids teaching the main session lifecycle about provider-specific title logic.

### 5. Derive a fallback title immediately

Every session gets a fallback title as soon as the daemon has user prompt text to summarize.

Fallback derivation rules:

1. Extract plain text from the first user prompt.
2. Concatenate text blocks in order and ignore non-text blocks.
3. Collapse internal whitespace to single spaces and trim leading and trailing space.
4. If no text remains, use `New session`.
5. Otherwise, keep the first 6 words without an ellipsis.
6. Enforce a hard cap of 48 characters, trimming to the last whole word that fits when possible.
7. Strip trailing punctuation such as `.`, `,`, `:`, `;`, `!`, and `?`.
8. If trimming empties the result, fall back to `New session`.

This fallback algorithm is deterministic, provider-free, and good enough to show immediately.

### 6. Generate a better title asynchronously when configured

Generation is attempted only when all of the following are true:

- a fallback title was derived from real prompt text;
- `sessionTitles.generator` is configured;
- the reusable resolver can load a runtime text model successfully;
- required credentials are available at runtime.

Trigger rules:

- If `initialPrompt` exists at creation time and title-generation config is usable, the daemon persists the fallback title and sets `titleState = "pending"`, then starts a background generation task.
- If `initialPrompt` exists but no usable generator config is present, the daemon persists the fallback title and sets `titleState = "fallback"`.
- If the session is created without `initialPrompt`, the daemon persists `title = "New session"` and `titleState = "placeholder"`.
- When the first later user prompt is accepted for a `placeholder` session, the daemon derives the fallback title and then chooses `pending` or `fallback` by the same generator-availability rules.
- There is at most one automatic title-generation attempt per session.

Recommended generation instruction contract:

- return exactly one line;
- 2 to 6 words;
- no quotes;
- no trailing punctuation;
- no markdown;
- reflect the task the user is asking for, not just the repository name.

### 7. Validate generated titles conservatively

When the background generation call returns:

- If the output is empty, multi-line, too long, obviously prompt-like, or otherwise invalid, keep the fallback title and set `titleState = "failed"`.
- If the output passes validation, replace `title` and set `titleState = "generated"`.
- If the normalized generated title equals the fallback title, accept it and still set `titleState = "generated"`.

Generation failure never changes session lifecycle state and never blocks later prompts.

## API / Interface Specification

### Shared Config

Add the new optional root-level section:

```json
{
  "sessionTitles": {
    "generator": {
      "provider": "openai",
      "model": "gpt-4.1-mini"
    }
  }
}
```

This config is independent from:

```json
{
  "session": {
    "model": "..."
  }
}
```

The first config controls title generation only. The second continues to control default agent model selection for new sessions.

### Session Output

Extend `DaemonSession` with:

```ts
type DaemonSession = {
  // existing fields
  title: string
  titleState: "placeholder" | "fallback" | "pending" | "generated" | "failed"
}
```

No new session-level `model` field is required for this feature.

### Host Behavior

- Session list rows and chat headers should always display `session.title`.
- If `titleState === "pending"`, the host may show a subtle loading affordance while continuing to display the fallback title.
- If `titleState === "fallback"`, the host should treat the current title as final unless a later refresh indicates otherwise.
- Hosts that care about fast pending-to-generated transitions should refresh `sessionGet` or `sessionList` while `titleState === "pending"`.
- A future settings surface should edit `sessionTitles.generator`, not `session.model`, when configuring title generation.

## Behavioral Semantics

### Order of Operations for Session Creation

1. Resolve title-generator config from root config.
2. Determine whether the title generator's text-model config is present and syntactically valid.
3. Derive the initial fallback title when `initialPrompt` text is available.
4. Create and initialize the real daemon session normally.
5. Persist the session record with `title` and `titleState`.
6. Return `sessionCreate`.
7. If `titleState === "pending"`, start the background title-generation task.

The background step must not delay `sessionCreate`.

### Sessions Without an Initial Prompt

If a session is created without `initialPrompt`:

- `title` starts as `New session`;
- `titleState` starts as `placeholder`;
- no title generation is attempted yet.

When the first user prompt is later accepted:

- derive the fallback title from that prompt;
- determine whether the title generator is usable;
- persist `titleState = "pending"` when generation can run, otherwise `titleState = "fallback"`;
- start the background generation task only in the `pending` case.

### Failure Behavior

- If no title-generator config is present, the daemon never attempts remote title generation and the fallback title remains in place.
- If generator config exists but credentials are missing or provider construction fails at runtime, the daemon keeps the fallback title and sets `titleState = "failed"`.
- If the daemon restarts while a title generation job is `pending`, reconciliation sets `titleState = "failed"` and keeps the fallback title.
- If no text blocks exist in the first prompt, the session stays on `New session` and no automatic title generation is attempted.

### Idempotency and Retry

- There is at most one automatic generation attempt per session.
- Automatic retries are not performed in v1.
- Manual retitle remains out of scope.

## Architecture / Data Flow

### Fresh Session with Initial Prompt and Configured Generator

1. Host calls `sessionCreate`.
2. Daemon resolves `sessionTitles.generator`.
3. Daemon derives the fallback title.
4. Daemon creates the real daemon session.
5. Daemon persists `title` and `titleState = "pending"`.
6. Daemon returns the session immediately.
7. Host shows the fallback title.
8. Daemon asks the reusable resolver module to load a runtime text model from JSON config.
9. Daemon runs one background provider-backed title-generation call through the title utility.
10. On success, daemon updates `title` and `titleState = "generated"`.
11. On failure, daemon keeps the fallback title and sets `titleState = "failed"`.

### Fresh Session with Initial Prompt and No Generator Config

1. Host calls `sessionCreate`.
2. Daemon derives the fallback title.
3. Daemon creates the real daemon session.
4. Daemon persists `title` and `titleState = "fallback"`.
5. Daemon returns the session.
6. No remote generation is attempted.

### Session Created Without an Initial Prompt

1. Host calls `sessionCreate` without `initialPrompt`.
2. Daemon persists `title = "New session"` and `titleState = "placeholder"`.
3. Later, the first user prompt is accepted.
4. Daemon derives the fallback title.
5. Daemon chooses `pending` or `fallback` based on generator availability.
6. If `pending`, daemon starts the background title-generation task.

## Alternatives and Tradeoffs

### Alternative: Heuristic Title Only

Rejected.

Using only the first few prompt words is cheap and deterministic, but it produces weak session labels for many prompts and does not satisfy the intended session-list UX when a provider-backed title model is available.

### Alternative: Generate Titles via Extra ACP Agent Sessions

Rejected.

Creating extra daemon or ACP sessions just to produce a short title adds avoidable process cost, persistence noise, and coupling between title generation and session lifecycle behavior.

### Alternative: Reuse `session.model`

Rejected.

`session.model` already has a separate meaning: the default agent model for new sessions. Reusing it for title generation would conflate two unrelated concerns and make configuration ambiguous.

### Alternative: Use Vercel `ai` for Title Generation

Accepted.

Rationale:

- title generation is a small, provider-facing utility boundary;
- the feature benefits from normalized provider behavior and output handling;
- `ai-sdk-json-schema` already provides the committed catalog, schema, and runtime helpers for this config shape;
- a reusable JSON-to-runtime resolver module can support future config-backed AI features, not just titles;
- it avoids polluting the ACP session subsystem with provider-specific logic;
- it supports a broad provider set through one abstraction.

Tradeoff:

- Goddard adds a direct model-provider dependency for this one auxiliary subsystem.
- In return, the title-generation path stays simple and avoids creating extra agent sessions.

## Failure Modes and Edge Cases

- Very long first prompt
  - The fallback title still resolves deterministically through local truncation rules.
- First prompt contains only images or non-text blocks
  - Title remains `New session`; no automatic generation is attempted.
- Generated title is identical to the fallback title
  - Accept it and mark `generated`.
- Generator config is absent
  - Stay on fallback behavior only.
- Generator config exists but API key env var is missing
  - Keep fallback title and mark `failed`.
- Generator config exists but provider is unknown
  - Keep fallback title and mark `failed`.
- Existing persisted sessions predate this feature
  - Backfill title fields during migration using `initiative` first, then first prompt fallback when available, then `New session`.

## Testing and Observability

### Tests

- Fallback title derivation from plain text prompts.
- Fallback derivation from mixed content blocks.
- Session created without initial prompt remains `placeholder` until the first user prompt.
- No configured generator yields `titleState = "fallback"` and no background title job.
- Configured generator yields `titleState = "pending"` and later `generated` on success.
- Missing credential or unknown provider yields `titleState = "failed"` and preserves the fallback title.
- Reconciliation converts abandoned `pending` titles to `failed`.
- Title generation path does not create a daemon session record or ACP traffic.
- The reusable resolver module maps persisted `{ provider, model }` JSON config into the expected runtime model instance for supported providers.

### Diagnostics

Emit session diagnostics for:

- `session_title_generation_started`
- `session_title_generated`
- `session_title_generation_failed`

These should include the session id and, when available, the configured provider and model id.

## Rollout / Migration

1. Extend session schemas and persistence with `title` and `titleState`.
2. Add `sessionTitles.generator` to shared config schema using `ai-sdk-json-schema`'s `textModelConfigSchema`, and update `goddard.json` generation to embed `textModelConfigJsonSchema` once under a local `$defs` entry referenced by `sessionTitles.generator`.
3. Backfill existing persisted sessions:
   - `initiative` when present;
   - otherwise fallback title derived from the first stored prompt when possible;
   - otherwise `New session`.
4. Add the reusable daemon resolver module that wraps `ai-sdk-json-schema` for JSON-to-runtime model loading.
5. Add the daemon title-generation utility using Vercel `ai` on top of that resolver module.
6. Add host refresh behavior for `pending` titles where quick replacement matters.

## Open Questions

- What is the smallest built-in provider set for the first implementation beyond an OpenAI-compatible path?
  - This is non-blocking as long as the config shape and provider registry seam are fixed first.
- What is the first app surface for editing `sessionTitles.generator`?
  - This is non-blocking because the shared config file can exist before a dedicated settings UI.

## Ambiguities and Blockers

- AB-1 - Resolved - Separation from `session.model`
  - Affected area: Proposed Design / Shared Config
  - Issue: Earlier drafts tied title generation to the session agent model.
  - Why it matters: That would conflate two unrelated configuration concerns.
  - Next step: Keep a separate `sessionTitles.generator` config surface.

- AB-2 - Resolved - Title generation execution boundary
  - Affected area: Proposed Design / Alternatives
  - Issue: Earlier drafts generated titles by creating extra ACP agent sessions.
  - Why it matters: That adds avoidable runtime and persistence complexity.
  - Next step: Use a daemon-internal provider utility based on Vercel `ai`.

- AB-3 - Non-blocking - First built-in provider registry set
  - Affected area: Rollout / Implementation
  - Issue: The seam is defined, but the exact initial built-in provider list is still open.
  - Why it matters: It affects implementation scope, not the contract.
  - Next step: Start with the smallest useful built-in set and keep the provider registry extensible.

- AB-4 - Resolved - Reusable provider resolution boundary
  - Affected area: Proposed Design / Rollout
  - Issue: JSON config resolution could have lived inside the title-generator module itself.
  - Why it matters: That would make future config-backed AI features reimplement the same JSON-to-runtime mapping logic.
  - Next step: Keep a dedicated daemon module responsible for mapping validated `ai-sdk-json-schema` text-model config into runtime AI SDK model instances.
