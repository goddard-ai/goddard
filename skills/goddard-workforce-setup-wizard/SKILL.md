---
name: goddard-workforce-setup-wizard
description: Initialize, inspect, explain, and troubleshoot Goddard repository workforce setup, including `.goddard/workforce.json`, `.goddard/ledger.jsonl`, `goddard-workforce` CLI flows, daemon runtime startup, and first request routing. Use when Codex needs to stand up workforce in a repository, choose domain agents from workspace packages, review or edit workforce ownership, start or inspect the runtime, or guide an operator through the first queued request.
---

# goddard-workforce-setup-wizard

Set up workforce as a daemon-owned, repository-scoped runtime. Preserve the existing CLI, config, and recovery model instead of inventing a parallel operator flow.

## Load The Right Context

- Read `workforce/README.md` for the operator model and first-use flow.
- Read `spec/daemon/workforce.md` before changing lifecycle assumptions or explaining guarantees.
- Read `workforce/src/main.ts` for supported commands, flags, and request intents.
- Read `core/daemon/src/workforce/config.ts` and `core/schema/src/workforce.ts` before editing `.goddard/workforce.json` manually.
- Read `core/sdk/README.md` when the user wants SDK-driven setup instead of CLI usage.

## Run The Setup Workflow

1. Resolve the repository root with `git rev-parse --show-toplevel`.
2. Check whether `.goddard/workforce.json` already exists before running init. Inspect and extend existing config instead of blindly overwriting it.
3. Discover likely domain boundaries from workspace `package.json` files, because the current initializer only considers package roots.
4. Recommend a root-plus-domain topology that matches package ownership and keeps ownership narrow.
5. Run the `goddard-workforce init` flow when the repository is not initialized yet.
6. Review the generated `.goddard/workforce.json` and confirm `cwd`, `owns`, and agent ids match the intended package boundaries.
7. Start the runtime explicitly with `goddard-workforce start`.
8. Inspect runtime state with `goddard-workforce status` or `goddard-workforce list`.
9. Queue a small first request only after the config looks correct.

## Preserve These Invariants

- Keep workforce daemon-owned. Do not describe clients as owning queue state or lifecycle.
- Keep exactly one runtime per repository root.
- Keep request handling sequential per agent.
- Keep the root agent repository-wide and domain agents narrow.
- Keep `.goddard/workforce.json` as durable intent and `.goddard/ledger.jsonl` as append-only history.
- Keep create-intent requests routed through the root agent.
- Call out that the app does not currently own a dedicated workforce UI. The current operator surface is CLI and SDK driven.

## Validate The Generated Config

- Confirm `version` stays `1`.
- Confirm `rootAgentId` points at an actual agent, usually `root`.
- Confirm the root agent owns `"."` and uses `cwd: "."`.
- Confirm each domain agent owns only the relative package paths it should handle.
- Confirm agent ids stay stable, lowercase, and package-shaped.
- Confirm the config does not duplicate ownership unless the user explicitly wants overlap and accepts the ambiguity.

## Use Command Shapes Deliberately

Use these shapes when the workspace already exposes the CLI binary:

```bash
goddard-workforce init --root /path/to/repo
goddard-workforce start --root /path/to/repo
goddard-workforce status --root /path/to/repo
goddard-workforce request --root /path/to/repo --target-agent-id root "Review the current queue."
goddard-workforce create --root /path/to/repo "Scaffold a new package for the app shell."
```

If `goddard-workforce` is not available in `PATH`, inspect the local workspace scripts and package build state before inventing an alternative invocation.

## Return A Useful Operator Summary

- Report the resolved repository root.
- Report whether workforce was newly initialized or already configured.
- Report the selected agents and owned paths.
- Report whether the runtime is running.
- Report the exact next command or first request that the operator should use.
