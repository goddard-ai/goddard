# `sprint-branch stop-sync`

The sprint-branch stop-sync command is part of the local sprint-branch workflow. This page explains when to use it, what sprint state or branches it may change, and what guardrails apply.

## Question it answers

- How do I stop a running `sprint-branch sync` process from another shell in
    the same working directory?

## Working directory selection

- The command targets `sync` processes started from the same resolved working
    directory.
- It does not infer a sprint and does not require `--sprint`.
- A different linked worktree, repository root, or subdirectory is treated as
    a different target.

## What it does

- Requests that each matching running `sync` command exit gracefully.
- Reports how many matching processes were asked to stop.
- Succeeds as a no-op when no matching `sync` process is running.
- `sprint-branch sync --replace` uses the same stop path before starting a
    replacement watch.

## What it changes

- It changes only the local control state used by running `sync` commands.
- The running `sync` command performs its normal review-sync cleanup before it
    exits.
- If it removes a stale control left by a dead `sync` process and the same
    worktree is still on a disposable `review-sync/*` branch, it restores the
    checkout that originally launched `sync` when that can be done without
    local changes.
- It does not advance sprint task state or mutate sprint branches directly.

## Guardrails

- It only targets commands that registered from the same resolved working
    directory.
- It ignores stale or invalid control records instead of treating them as
    active sync processes.
- It leaves the current checkout untouched when the worktree has local
    changes or the stale record does not contain the starting checkout.
