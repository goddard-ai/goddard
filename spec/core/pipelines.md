# Pipelines

Pipelines are reusable linear handoff workflows made of ordered steps. They let humans and integrations spawn a repeatable run that may combine agents, registered transformers, and approval gates without treating the whole workflow as one chat session.

## Definitions

A Pipeline definition is a registered capability. It declares a stable identity, version, human-facing name, accepted inputs, and an ordered list of steps.

Project-local Pipeline definitions live under `.goddard/pipelines/`. User or plugin-provided definitions may also be registered, but repository-local definitions are the shared project authoring surface.

Definitions are reusable intent, not runtime state. Changing a definition must not rewrite the historical meaning of runs that already started.

## Runs

A Pipeline run is one daemon-managed instance of a Pipeline definition with concrete runtime inputs. Runs preserve enough definition identity and step shape for later inspection even if the reusable definition changes.

The daemon owns Pipeline run lifecycle state. The app, SDK, and approved operational clients may spawn, observe, cancel, retry, or resume runs through daemon-backed control surfaces, but they must not create parallel ownership of mutable run state.

Pipeline runs use this lifecycle:

`Queued -> Running -> Waiting -> Succeeded | Failed | Cancelled`

`Waiting` represents an explicit approval or human-input gate. A waiting run must not continue until an authorized client resumes it.

## Linear Execution

Pipeline v1 is linear handoff only. Steps execute in the order declared by the definition.

A step may read:

- the Pipeline run inputs
- outputs from earlier steps

A step must not read outputs from later steps. V1 does not support branching, fan-out, joins, loops, or parallel execution.

## Step Kinds

V1 supports these step kinds:

- `script`: runs a registered transformer capability and persists its output.
- `agent`: creates daemon-managed agent work for a step and persists the step result.
- `approval`: pauses the run until an authorized client resumes it.

Script steps must reference registered transformer capabilities. Persisted Pipeline configuration must not grant a repository the ability to execute arbitrary local code.

Agent steps may create underlying daemon sessions. Those sessions are implementation details of the Pipeline run unless a client explicitly opens them for inspection.

## Session Visibility

Sessions created by the app for ordinary user work are visible in the primary Sessions workflow by default.

Sessions created by SDK hosts, CLIs, playgrounds, automations, or Pipeline steps carry provenance that lets clients distinguish their source. Pipeline-created agent sessions are hidden from the primary Sessions list by default so repeatable workflows do not pollute ordinary session history.

Hidden sessions may still be addressable by authorized daemon, SDK, or app surfaces when a Pipeline run links to them for diagnostics or detailed inspection.

## App And SDK Boundaries

The SDK is the programmatic control surface for listing Pipeline definitions, spawning runs, inspecting progress, cancelling runs, retrying failed steps, and resuming waiting steps.

The app is the primary human-facing surface for Pipeline progress. It should present a Pipeline run as ordered step progress with inspectable inputs, outputs, errors, and linked diagnostics, not as one ordinary chat transcript.

Shared Pipeline data loading, mutation, visibility, and system configuration behavior must remain aligned across daemon, SDK, and app surfaces.
