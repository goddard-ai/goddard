# Daemon-Backed Operational CLI

## Goal
Support a narrow command-line surface for initializing repository-local automation intent and controlling daemon-backed local runtimes when operators need terminal access.

## Hypothesis
We believe that a thin operational CLI will lower friction for automation setup and recovery without undermining the desktop app as the primary human-facing workspace.

## Actors
- Operator managing local automation bring-up, inspection, or recovery
- Daemon-backed local runtime receiving lifecycle and mutation requests
- SDK-backed CLI implementation reusing shared contracts

## Supported Intent
- Initialize repository-local automation state needed before runtime control begins.
- Start and stop supported daemon-backed runtimes.
- Inspect current daemon-managed runtime status.
- Submit or mutate operational commands that belong to daemon-managed automation domains.

## Constraints
- The CLI must remain a thin operator surface over shared SDK and daemon contracts.
- Runtime lifecycle authority remains with the daemon, not with the CLI process.
- CLI behavior must stay operational rather than evolving into a broad interactive workspace.
- Any supported CLI capability must stay aligned with the same underlying capability in the SDK.

## Non-Goals
- Becoming the primary review, planning, or spec-authoring surface
- Defining command flags, output formatting, or shell ergonomics in this spec
- Serving as a workaround for capabilities that should instead live in the desktop app or SDK

## Decision Memory
A narrow operational CLI remains justified where operators need direct local control over daemon-backed automation, but it must stay thin so Goddard does not reintroduce a parallel terminal-first product surface.
