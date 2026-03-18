# @goddard/worktree

A utility for managing Git worktrees with pluggable strategies.

## Purpose

The `Worktree` class provides a consistent interface for creating and cleaning up Git worktrees. It supports different "plugins" to determine how a worktree should be set up, falling back to a default implementation if necessary. This is primarily used to isolate development environments for automated agents.

## Usage Example

```typescript
import { Worktree } from '@goddard/worktree';

const worktree = new Worktree({
  cwd: '/path/to/repo',
});

// Setup a new worktree for a branch
const { worktreeDir, branchName } = worktree.setup('feature-branch');

console.log(`Worktree created at: ${worktreeDir}`);

// ... perform operations in worktreeDir ...

// Clean up when finished
worktree.cleanup(worktreeDir, branchName);
```

## License

This project is licensed under the [MIT License](./LICENSE-MIT).
