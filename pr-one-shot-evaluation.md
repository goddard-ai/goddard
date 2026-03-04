# One-Shot Pi Sessions for PR Feedback

## Current Implementation

Currently, when the Goddard daemon receives a PR feedback event (a `comment` or `review` on a managed PR), it spawns a one-shot `pi` process in the foreground.

The logic is housed in `daemon/src/index.ts`. When a relevant event is detected, the daemon triggers `defaultRunOneShot`:

```typescript
function defaultRunOneShot(input: OneShotInput): number {
  const result = spawnSync(input.piBin, [input.prompt], {
    cwd: input.projectDir,
    stdio: "inherit"
  });
  return result.status ?? 1;
}
```

### Issues with the Current Approach

1.  **Blocking execution**: `spawnSync` is used, meaning the daemon blocks while the `pi` process runs. It also uses `stdio: "inherit"`, which hijacks the terminal running the daemon.
2.  **Shared workspace**: The process runs directly in `input.projectDir`. If multiple PRs receive feedback simultaneously, or if a user is actively working in the project directory, they will step on each other's toes regarding git branches and uncommitted changes.
3.  **State management**: There is no isolation. Running `pi` could manipulate the main working tree directly.

## Proposed Architecture: Worktrees and Tmux

To achieve isolated, concurrent one-shot sessions that don't block the main daemon terminal, we should implement a pattern utilizing Git worktrees and Tmux sessions.

### 1. Git Worktrees

Instead of running in `input.projectDir` directly, the daemon should create a Git worktree for the specific PR branch.

-   **Path**: Something like `.worktrees/pr-<number>` or `/tmp/goddard-worktrees/<repo>-pr-<number>`.
-   **Benefits**: Gives the `pi` instance a clean, isolated working directory checked out to the exact PR branch without altering the main project directory's state.

### 2. Tmux Sessions

Instead of inheriting standard I/O in a blocking manner, we should spawn the `pi` process within a detached Tmux session.

-   **Command**: `tmux new-session -d -s pi-pr-<number>-<timestamp> -c <worktree-path> "pi '<prompt>'"`
-   **Benefits**:
    -   Non-blocking for the daemon.
    -   Keeps the `pi` session running in the background.
    -   Allows human operators to easily attach to the session (`tmux attach -t <session_name>`) to observe or intervene if the agent gets stuck.

### Implementation Steps

1.  Update `defaultRunOneShot` to execute asynchronous child processes (`spawn` instead of `spawnSync`) or handle the blocking nature externally. (Since tmux is detached, `spawnSync` on the tmux command itself would return immediately anyway).
2.  Add logic to retrieve the correct branch name for the PR (may require a GitHub API call if not provided in the event payload).
3.  Add logic to create the git worktree: `git worktree add <path> <branch>`.
4.  Construct and execute the `tmux` command.
5.  Add cleanup logic to prune worktrees when the session completes or the PR is merged/closed.