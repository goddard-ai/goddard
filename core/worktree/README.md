# @goddard-ai/worktree

A utility for managing Git worktrees with pluggable strategies.

## Purpose

The package provides top-level helpers for resolving plugins and creating or deleting Git worktrees. It supports different "plugins" to determine how a worktree should be set up, falling back to a default implementation if necessary. This is primarily used to isolate development environments for automated agents.

## Usage Example

```typescript
import {
  createWorktree,
  deleteWorktree,
  resolveWorktreePlugin,
} from "@goddard-ai/worktree"

const plugin = await resolveWorktreePlugin({
  cwd: "/path/to/repo",
})

console.log(`Worktree plugin: ${plugin.name}`)

// Create a new worktree for a branch.
const { effectiveCwd, worktreeDir, branchName, poweredBy } = await createWorktree({
  cwd: "/path/to/repo",
  requestedCwd: "/path/to/repo/packages/example",
  branchName: "feature-branch",
})

console.log(`Worktree created at: ${worktreeDir}`)
console.log(`Start working in: ${effectiveCwd}`)

// ... perform operations in worktreeDir ...

// Clean up when finished.
await deleteWorktree({
  cwd: "/path/to/repo",
  worktreeDir,
  branchName,
  poweredBy,
})
```

## License

This project is licensed under the [MIT License](./LICENSE-MIT).
