# `sprint-branch restore-sprint [--sprint <name>] [--force]`

The sprint-branch restore-sprint [--sprint <name>] [--force] command is part of the local sprint-branch workflow. This page explains when to use it, what sprint state or branches it may change, and what guardrails apply.

## Question it answers

- How can an accidentally removed `sprints/<name>/` folder be restored from
    the latest local backup?

## Inputs and selection

- Uses [standard sprint selection](../sprint-selection.md).
- `--force` replaces an existing sprint folder with the latest backup.

## What it does

- Restores task markdown files from the latest Git-private backup for the
    selected sprint.
- Restores into the sprint worktree recorded by private sprint state when
    that state is readable.
- If private sprint state is missing, restores into the current worktree.
- Does not recreate, rewrite, or move sprint branch state.
- Does not move Git branches.

## Backup source

- Successful sprint state writes keep one latest local backup of
    `sprints/<name>/` while that folder exists.
- Backups are local Git-private metadata and are not review history.
- Only the latest backup is retained.

## Guardrails

- Requires an existing local backup.
- Refuses to overwrite an existing sprint folder unless `--force` is passed.
- If both the live sprint folder and backup are missing, restore is not
    possible.

## Dry run

- Reports the backup and destination path.
- Does not write working tree files.

## Why it exists

- `sprints/` is intentionally ignored by Git so sprint plans stay out of
    review diffs.
- The restore command covers accidental local folder loss without changing
    the branch workflow.
