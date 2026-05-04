# `sprint-branch` Procedures

This directory describes what `sprint-branch` supports at a conceptual level.
It is written for agents and humans, including people who need to understand the
sprint review process without reading source code.

These pages intentionally avoid implementation details. They describe outcomes,
guardrails, and procedural shape, not internal files, schemas, locks, or command
implementation.

## Procedure Index

- [Sprint branch model](./model.md): the durable concepts behind branch roles,
  task states, sprint selection, dry runs, and clean-working-tree guardrails.
- [Standard sprint workflow](./standard-sprint-workflow.md): the normal
  end-to-end procedure from setup through cleanup, including feedback and
  recovery entry points.

## Command Procedures

| Command | Primary audience | Mutates state or branches | Purpose |
| --- | --- | --- | --- |
| [`status`](./commands/status.md) | Agents and humans | No | Inspect sprint branch state and the next safe action. |
| [`diff`](./commands/diff.md) | Agents and humans | No | Show the review delta against approved work. |
| [`view`](./commands/view.md) | Humans, agents preparing review | No | Print the approval packet for a finished task. |
| [`sync`](./commands/sync.md) | Humans reviewing agent work | Delegates to review sync | Watch the active review branch through `review-sync`. |
| [`doctor`](./commands/doctor.md) | Agents and humans | No | Diagnose inconsistent sprint state and recovery direction. |
| [`list`](./commands/list.md) | Agents and humans | No | List known active or parked sprints. |
| [`checkout`](./commands/checkout.md) | Humans | Checks out a detached snapshot | Inspect a review branch without taking branch ownership. |
| [`land`](./commands/land.md) | Humans | Yes | Fast-forward a target branch to finalized sprint work. |
| [`cleanup`](./commands/cleanup.md) | Humans | Yes | Remove landed sprint branches, review worktrees, and state. |
| [`init`](./commands/init.md) | Agents setting up a sprint | Yes | Create the branch scaffold and initial sprint state. |
| [`reset-state`](./commands/reset-state.md) | Agents recovering or replanning | Yes, state only | Rebuild private sprint state around a selected next task. |
| [`park`](./commands/park.md) | Agents and humans | Yes, state only | Hide a sprint from default active selection. |
| [`unpark`](./commands/unpark.md) | Agents and humans | Yes, state only | Restore a parked sprint to default active selection. |
| [`start`](./commands/start.md) | Agents | Yes | Assign the next planned task to `review` or `next`. |
| [`finish`](./commands/finish.md) | Agents | Yes, state only | Mark an active task ready for human review. |
| [`feedback`](./commands/feedback.md) | Agents | Yes | Interrupt work-ahead and return to `review`. |
| [`resume`](./commands/resume.md) | Agents | Yes | Return to interrupted or dependent `next` work. |
| [`approve`](./commands/approve.md) | Agents after human approval | Yes | Promote reviewed work into `approved` and roll the queue forward. |
| [`rebase`](./commands/rebase.md) | Agents | Yes | Move the whole sprint branch stack onto a new target ref. |
| [`finalize`](./commands/finalize.md) | Agents | Yes | Prepare fully approved sprint work for human landing. |
