# Session Worktree Bootstrap Design

Status: proposed

## Overview

Daemon-managed session worktrees are isolated, but a fresh checkout is often not immediately usable. Repositories commonly need a dependency install and benefit from reusing local untracked artifacts such as `node_modules`, `dist`, or `.turbo`.

This design adds a daemon-owned preparation phase for freshly created session worktrees. The phase has two parts:

1. Best-effort seeding of selected untracked artifacts from the source checkout into the new worktree using copy-on-write when available.
2. An automatic bootstrap step that runs a package-manager install command in the new worktree.

The default behavior is zero-config for common JavaScript and Bun repositories. Repositories may override the defaults declaratively in `.goddard/config.json`. The design does not add arbitrary repo-local shell hooks.

## Context

Today the daemon can create an isolated `Session Worktree` when a caller opts in with `worktree.enabled: true`. The session manager creates the worktree before the agent starts and persists its metadata on the session.

The current persisted root config exposes `worktrees.defaultFolder` and global-only `worktrees.plugins`. Repository-local config cannot declare plugins. That trust model should remain unchanged.

This feature targets freshly created worktrees owned by the built-in `default` worktree plugin. It does not change reuse semantics for existing worktrees.

## Goals

- Prepare fresh default-plugin session worktrees automatically before the agent starts.
- Provide a zero-config path that infers the package manager from repository metadata.
- Reuse high-value untracked artifacts when the new worktree starts from the same `HEAD` commit as the source checkout.
- Allow repository-local declarative overrides in `.goddard/config.json`.
- Keep global-only custom worktree plugin loading unchanged.

## Non-Goals

- Supporting arbitrary repo-local shell commands or executable hooks.
- Validating that copied artifacts are fresh or compatible beyond the same-`HEAD` gate.
- Bootstrapping reused worktrees during `loadSession()`.
- Changing the `@goddard-ai/worktree-plugin` contract in v1.
- Providing inferred bootstrap for non-JavaScript ecosystems in v1.

## Success Criteria

- A fresh session worktree in a recognized Bun or Node repository becomes usable without a manual install step.
- A fresh worktree created from the source checkout's current `HEAD` reuses configured untracked artifacts when those artifacts exist.
- A repository can override package-manager inference and seed inputs with machine-readable local config.
- Worktree setup failures surface as deterministic launch behavior rather than silent partial setup.

## Assumptions and Constraints

- The repository is pre-alpha. Backward compatibility is not required.
- The daemon already resolves merged root config before launch and local config overrides global config.
- Arrays in merged config replace earlier arrays instead of concatenating.
- Repository-local config remains a collaboration boundary, but custom worktree plugins stay global-only.
- Running a package-manager install implies normal repository trust, including any install-time scripts executed by that package manager.
- Seeding is a performance optimization, not a correctness guarantee.

## Terminology

- `Source Checkout`
  - The original repository checkout from which the session launch was requested.
- `Fresh Session Worktree`
  - A new isolated checkout created during `newSession()`, not a previously persisted worktree reused by `loadSession()`.
- `Seed Candidate`
  - One untracked file or directory in the source checkout that is eligible to be copied into a fresh worktree.
- `Bootstrap`
  - The daemon-owned install step that prepares a fresh worktree after creation and optional seeding.
- `Same-HEAD Worktree`
  - A fresh worktree whose resolved `HEAD` commit equals the source checkout's resolved `HEAD` commit immediately after worktree creation.

## Proposed Design

### Scope

The daemon adds a post-create preparation step for fresh session worktrees only when all of the following are true:

- `request.worktree.enabled === true`
- the session launch resolves a git repository root
- the daemon creates a fresh worktree instead of reusing an existing one
- the resulting worktree was created by the built-in `default` plugin

Worktrees created by `worktrunk` or custom plugins skip the daemon-owned preparation phase in v1. Those plugins continue to own their own setup semantics.

### Config Surface

The root config gains a new optional `worktrees.bootstrap` object that is valid in both global and local config:

```json
{
  "worktrees": {
    "bootstrap": {
      "enabled": true,
      "packageManager": "pnpm",
      "installArgs": ["--frozen-lockfile"],
      "seedEnabled": true,
      "seedNames": ["node_modules", "dist", ".turbo"],
      "seedPaths": ["app/styled-system/dist"]
    }
  }
}
```

The fields behave as follows:

- `enabled?: boolean`
  - Defaults to `true`.
  - When `false`, the daemon skips both seeding and bootstrap.
- `packageManager?: "bun" | "pnpm" | "npm" | "yarn"`
  - Overrides package-manager inference.
  - The daemon still runs a daemon-owned command, not an arbitrary shell string.
- `installArgs?: string[]`
  - Optional extra arguments appended to the daemon-owned install command.
  - Arguments are passed directly to the package-manager process without shell interpolation.
- `seedEnabled?: boolean`
  - Defaults to `true`.
  - When `false`, the daemon skips seeding but may still run bootstrap.
- `seedNames?: string[]`
  - Optional recursive basename allowlist for untracked artifacts to seed.
  - Defaults to `["node_modules", "dist", ".turbo"]`.
- `seedPaths?: string[]`
  - Optional exact repo-relative file or directory paths to seed in addition to `seedNames`.
  - Defaults to `[]`.

Config merge semantics follow the existing root-config rules:

- local config overrides global config
- object fields merge by key
- arrays replace earlier arrays

That means a local `seedNames` array replaces, not extends, the global `seedNames` array.

### Package-Manager Inference

When `worktrees.bootstrap.packageManager` is absent, the daemon infers the package manager from the repository root using this order:

1. `package.json#packageManager`, if present and recognized
2. one recognized lockfile at repository root

Recognized values are `bun`, `pnpm`, `npm`, and `yarn`.

Inference rules:

- If `package.json#packageManager` is present, the daemon uses the name prefix before `@`.
- If no recognized `packageManager` field is present, the daemon inspects repository-root lockfiles.
- If exactly one recognized lockfile is present, the daemon uses that tool.
- If zero recognized lockfiles are present, inferred bootstrap is skipped.
- If multiple recognized lockfiles are present, inferred bootstrap is skipped as ambiguous.

The daemon-owned commands are:

- `bun install`
- `pnpm install`
- `npm install`
- `yarn install`

If `installArgs` is configured, those arguments are appended to the selected command.

### Seeding Model

Seeding is best-effort and non-fatal. The daemon attempts seeding before bootstrap.

Seeding runs only for `Same-HEAD Worktree` instances. The daemon determines this by resolving `git rev-parse HEAD` in:

- the source checkout repository root
- the fresh worktree repository root

If the two commit ids differ, the daemon skips seeding and proceeds directly to bootstrap.

This commit-equality rule is the only freshness gate in v1. The daemon does not inspect lockfile hashes, runtime versions, or package-manager state.

### Seed Candidate Resolution

The daemon constructs seed candidates from two sources:

- recursive basename matches for every entry in `seedNames`
- exact repo-relative paths in `seedPaths`

Candidate rules:

- The candidate must resolve inside the repository root.
- The candidate must exist in the source checkout.
- Git must treat the entire candidate path as untracked in the source checkout.
- Existing target paths in the fresh worktree are not overwritten.
- Duplicate candidates are deduplicated by normalized repo-relative path.

If a candidate path is tracked, partially tracked, ignored, or outside the repository root, the daemon skips it.

The daemon copies each eligible candidate from the source checkout to the fresh worktree using copy-on-write when the filesystem supports it, and falls back to a normal copy when it does not. Failure to copy one candidate does not abort the session launch.

### Bootstrap Model

Bootstrap runs after seeding, if `enabled !== false` and a package manager was either configured or inferred.

Bootstrap semantics:

- The command runs at the fresh worktree repository root, not at `requestedCwd`.
- The command runs exactly once for a fresh worktree creation.
- Reused worktrees skip bootstrap.
- If no package manager can be resolved, bootstrap is skipped without failing launch.
- If a package manager is resolved but the process cannot be started or exits non-zero, session launch fails.

Failing launch on an actual bootstrap failure is intentional. Once the daemon has decided a worktree needs a package-manager install, silently continuing with a half-prepared environment is less predictable than surfacing the failure immediately.

## API and Interface Specification

### Root Config Schema

`WorktreesConfig` gains:

```ts
type WorktreesBootstrapConfig = {
  enabled?: boolean
  packageManager?: "bun" | "pnpm" | "npm" | "yarn"
  installArgs?: string[]
  seedEnabled?: boolean
  seedNames?: string[]
  seedPaths?: string[]
}

type WorktreesConfig = {
  defaultFolder?: string
  plugins?: WorktreePluginReference[]
  bootstrap?: WorktreesBootstrapConfig
}
```

Validation constraints:

- `installArgs`, `seedNames`, and `seedPaths` must be arrays of non-empty strings.
- `seedPaths` must be repository-relative at runtime. Absolute or escaping paths are invalid for execution and are skipped with diagnostics.
- `plugins` remains global-only. `bootstrap` is allowed in both global and local config.

### Daemon Runtime Boundary

No session creation API changes are required in v1. `CreateSessionRequest.worktree` remains:

```ts
type SessionWorktreeParams = {
  enabled?: boolean
}
```

The daemon derives worktree bootstrap behavior from merged root config plus repository metadata, not from per-request runtime overrides.

### Internal Module Boundary

The daemon adds an internal preparation module, for example `core/daemon/src/worktrees/bootstrap.ts`, with responsibilities to:

- resolve the merged bootstrap config for a repository
- infer the package manager when not configured
- evaluate the same-`HEAD` seeding gate
- resolve and copy seed candidates
- run the bootstrap install command
- emit diagnostics and structured logs

The `@goddard-ai/worktree-plugin` contract does not change in v1.

## Behavioral Semantics

### Fresh Worktree Launch Flow

1. `Session Manager` resolves merged root config.
2. The daemon creates a fresh session worktree using the existing worktree resolution flow.
3. If the created worktree is not powered by `default`, skip the preparation phase.
4. Resolve `worktrees.bootstrap` from merged config. If absent, use defaults.
5. If `enabled === false`, skip the preparation phase.
6. If `seedEnabled !== false`, compare source and target `HEAD` commits.
7. If the commits match, resolve and copy eligible seed candidates.
8. Resolve the package manager from config or inference.
9. If no package manager is resolved, finish preparation successfully.
10. Run the package-manager install command in the fresh worktree repository root.
11. If the command succeeds, continue normal session initialization.
12. If the command fails, abort session launch and surface the bootstrap failure.

### Reused Worktree Flow

When `loadSession()` reuses a persisted worktree:

- the daemon revalidates the worktree as it does today
- the daemon does not re-run seeding
- the daemon does not re-run bootstrap

### Conflict Resolution

- Explicit `worktrees.bootstrap.packageManager` overrides inferred package-manager detection.
- Explicit `seedNames` replaces default or inherited `seedNames`.
- Explicit `seedPaths` replaces inherited `seedPaths`.
- Seeding never overwrites an existing target path in the fresh worktree.
- Bootstrap is independent from seeding. A seeding skip or seeding copy failure does not skip bootstrap.

## Architecture and End-to-End Flow

The feature fits into the current daemon path as follows:

- `Session Manager`
  - still decides whether a session uses a worktree
  - creates the fresh worktree through the existing plugin path
  - invokes the new preparation module only for fresh `default`-plugin worktrees
- `Worktree Bootstrap Module`
  - reads the merged bootstrap config
  - computes the same-`HEAD` gate
  - copies eligible untracked artifacts
  - runs the package-manager install
- `Config Manager`
  - continues to own config loading, validation, and last-good snapshots
- `Diagnostics / Logging`
  - records the preparation plan, seed skip reasons, copy outcomes, and bootstrap result

The end-to-end result is:

- fresh default-plugin worktree
- optional seeded caches or build artifacts
- package-manager install performed at worktree repo root
- agent starts only after bootstrap succeeds or was intentionally skipped

## Alternatives and Tradeoffs

### Full Worktree CoW Copy

Rejected for this design. Copying the entire source checkout is broader than the actual requirement and carries more stale-state risk. The new design copies only selected untracked artifacts and keeps worktree creation separate from preparation.

Tradeoff:

- less raw reuse than a whole-tree copy
- better control over what crosses worktree boundaries

### Bootstrap Only, No Seeding

Rejected. It is simpler, but it leaves useful local build artifacts and dependency trees on the table for the common same-`HEAD` case.

Tradeoff:

- seeding adds some complexity
- warm-start worktrees are materially faster in common repos

### Repo-Local Arbitrary Shell Hooks

Rejected in v1. Arbitrary repo-local commands are flexible but broaden the trust surface, complicate portability, and make behavior harder to reason about and validate.

Tradeoff:

- less flexibility for unusual repositories
- more predictable, reviewable, and machine-readable behavior

### Extending the Worktree Plugin Interface

Rejected in v1. Adding plugin hooks for seeding or bootstrap would expand the public contract before the daemon-owned behavior is proven useful.

Tradeoff:

- less plugin flexibility now
- smaller and safer initial design

## Failure Modes and Edge Cases

- Repository is not a git repository:
  - Existing behavior applies. No worktree is created, so no bootstrap runs.
- Fresh worktree created by `worktrunk` or a custom plugin:
  - Preparation is skipped in v1.
- Source and target `HEAD` differ:
  - Seeding is skipped. Bootstrap may still run.
- Config contains an invalid bootstrap object:
  - The config manager keeps the last good config snapshot, consistent with existing daemon behavior.
- `seedPaths` entry escapes the repository root:
  - That entry is skipped and recorded in diagnostics.
- Candidate is tracked, partially tracked, or ignored:
  - That candidate is skipped.
- Copy-on-write is unavailable:
  - The daemon falls back to a normal copy for that candidate.
- A copy operation fails:
  - The daemon records the failure and continues.
- Package manager cannot be inferred:
  - Bootstrap is skipped.
- Package manager is resolved but binary is missing or install exits non-zero:
  - Session launch fails with a bootstrap error.

## Testing and Observability

### Tests

The implementation should add tests for:

- schema validation for `worktrees.bootstrap`
- root-config merge behavior, especially array replacement
- package-manager inference from `packageManager`
- package-manager inference from a single lockfile
- ambiguous lockfile detection
- same-`HEAD` seeding gate
- seed candidate filtering for tracked and untracked paths
- seed candidate path normalization and escape rejection
- skipping preparation for reused worktrees
- skipping preparation for non-`default` plugins
- bootstrap success and failure launch behavior

### Observability

The daemon should emit structured logs and session diagnostics for:

- preparation started
- preparation skipped, including the skip reason
- each copied or skipped seed candidate
- inferred or configured package-manager selection
- bootstrap command success
- bootstrap command failure

Diagnostics should include `repoRoot`, `worktreeDir`, `poweredBy`, and the selected package manager when one exists.

## Rollout and Migration

No persisted data migration is required.

Rollout plan:

1. Add schema and config merge support for `worktrees.bootstrap`.
2. Implement the daemon-owned preparation module.
3. Invoke preparation for fresh `default`-plugin worktrees in session launch.
4. Add diagnostics and tests.

Because the feature is pre-alpha and config-driven, rollback is straightforward:

- disable `worktrees.bootstrap.enabled`
- or revert the daemon behavior without touching session persistence

## Open Questions

- Should the desktop app eventually surface explicit worktree preparation status in session UI, or are diagnostics and logs sufficient for v1?

## Ambiguities and Blockers

- AB-1 - Resolved - Same-branch vs same-`HEAD` seeding gate
  - Affected area: Behavioral Semantics
  - Issue: Branch-name equality is not precise enough to determine whether copied untracked artifacts are likely to be reusable.
  - Why it matters: The daemon needs one deterministic rule for when seeding is allowed.
  - Next step: Use exact `HEAD` commit equality between source checkout and fresh worktree.

- AB-2 - Deferred - Repo-local arbitrary bootstrap commands
  - Affected area: Config Surface / Trust Model
  - Issue: Some repositories may eventually want custom setup beyond package-manager install.
  - Why it matters: Supporting arbitrary commands increases flexibility but broadens the repo-local execution surface.
  - Next step: Defer until the daemon-owned package-manager bootstrap path proves insufficient.

## Appendix

### Example Zero-Config Behavior

For a repository with:

- `package.json` containing `"packageManager": "bun@1.3.11"`
- a fresh `default`-plugin session worktree
- `HEAD` equality between source checkout and target worktree

the daemon will:

1. seed untracked `node_modules`, `dist`, and `.turbo` paths when present
2. run `bun install` at the fresh worktree repository root
3. start the agent only after `bun install` succeeds

### Example Local Override

```json
{
  "worktrees": {
    "bootstrap": {
      "packageManager": "pnpm",
      "installArgs": ["--frozen-lockfile"],
      "seedNames": ["node_modules", ".turbo"],
      "seedPaths": ["core/daemon/dist"]
    }
  }
}
```

This forces `pnpm install --frozen-lockfile`, removes `dist` from recursive basename seeding, and adds one explicit repo-relative seed path.
