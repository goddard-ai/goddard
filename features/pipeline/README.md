# Pipeline Feature

The pipeline feature owns reusable linear Pipeline definition contracts, project-local definition loading, daemon-managed run state, SDK access, and app metadata for Pipeline progress surfaces.

Exports:

- `@goddard-ai/pipeline/schema`: Pipeline definition, step, and status schemas.
- `@goddard-ai/pipeline/loader`: project-local `.goddard/pipelines/` discovery, validation, and diagnostics.
- `@goddard-ai/pipeline/daemon`: the daemon plugin factory, default daemon plugin, run manager, and registry helpers.
- `@goddard-ai/pipeline/daemon-ipc`: the Pipeline IPC route contract.
- `@goddard-ai/pipeline/sdk`: the Pipeline SDK namespace plugin.
- `@goddard-ai/pipeline/app`: static app contribution metadata for Pipelines navigation and run tabs.

## Definition Authoring

Project-local definitions live in one of these locations:

- `.goddard/pipelines/<pipeline-id>/pipeline.yaml`
- `.goddard/pipelines/<pipeline-id>.yaml`

Directory definitions are preferred when agent prompt files are involved. Agent `systemPromptFile` paths are resolved relative to the definition file.

A definition declares:

- `id`: kebab-case stable identifier.
- `version`: version string used to disambiguate multiple registered definitions.
- `name`: human-facing label.
- `description`: optional summary.
- `inputs`: declared runtime input keys.
- `steps`: ordered linear steps.
- `outputs`: optional output metadata.

Pipeline v1 is linear handoff only. Step input mappings may reference `$.inputs.<key>` or earlier step outputs such as `$.steps.<step-id>.output.<path>`. Mappings cannot reference later steps.

Supported step kinds:

- `script`: calls a registered transformer by name and persists its returned output.
- `agent`: creates a hidden one-shot daemon session and persists its result.
- `approval`: pauses the run until an authorized client approves it.

Script steps intentionally do not execute arbitrary repository code. Host packages register named transformer functions with `createPipelinePlugin({ transformers })`.

## Runtime Surfaces

The daemon owns Pipeline run and step lifecycle state. SDK and app clients call daemon-backed IPC methods instead of mutating Pipeline state directly.

The SDK namespace is `sdk.pipeline` and exposes:

- `listDefinitions`
- `listDefinitionDiagnostics`
- `spawnRun`
- `getRun`
- `listRuns`
- `cancelRun`
- `advanceRun`
- `approveRun`
- `retryRun`

The app contributes:

- A primary Pipelines workbench surface for definitions, diagnostics, run spawning, and recent runs.
- A closable Pipeline run tab that shows ordered step status, inputs, outputs, errors, and linked agent session ids.

Agent sessions created by Pipeline steps use `origin: "pipeline"` and `visibility: "hidden"` so primary session history remains focused on user-created sessions.
