# Fresh Session Worktree Preparation

Fresh isolated daemon sessions can prepare a newly created worktree for agent execution without requiring each repository or operator to repeat the same manual bootstrap work.

## Participants
- Repository Maintainer — declares shared preparation intent for the repository.
- Operator — launches or approves daemon-managed sessions that may use isolated worktrees.
- Daemon Runtime — decides whether preparation applies and performs it before session start.
- Agent Session — starts against the prepared worktree once launch begins.

## State Model

`WorktreeCreated -> PreparationEvaluated -> (Seeded | SeedingSkipped) -> (Bootstrapped | BootstrapSkipped) -> Ready`

## Capabilities
- A fresh isolated worktree created for a daemon-managed session may be prepared before the agent starts.
- Preparation exists to improve startup speed and reduce manual setup work. It is not the source of truth for repository correctness.
- Preparation has two conceptual phases when eligible:
  1. Reuse of selected untracked repository artifacts from the source checkout into the fresh worktree.
  2. Repository bootstrap so the worktree can satisfy normal dependency or tool setup expectations.
- Reuse of untracked artifacts is allowed only when the fresh worktree starts from the same commit as the source checkout it was created from.
- When the new worktree starts from a different commit, the daemon skips artifact reuse and may still perform repository bootstrap.
- Preparation policy is repository-scoped intent so collaborators can share a zero-config default for common setup needs.
- Sync-enabled isolated sessions apply preparation before the sync mount is established, so the prepared state becomes part of the shared mounted view by design.
- Automatic bootstrap may be inferred when repository intent is absent, but only when the daemon can determine the repository's package-management intent without ambiguity.

## Boundaries
- Preparation applies only to newly created isolated worktrees managed by the daemon's built-in worktree path.
- Preparation does not retroactively mutate reused worktrees or redefine the behavior of custom worktree providers.
- Artifact reuse is allowlisted repository intent, not a blanket copy of all untracked content.
- Failure to reuse artifacts must not prevent later bootstrap or session start by itself.
- If the daemon resolves package-management bootstrap unambiguously and that bootstrap fails, the session launch must fail rather than starting from a partially prepared state.
- Repository-local configuration may shape preparation policy, but repository-local executable extensions must not be able to replace daemon worktree creation or run arbitrary trusted provisioning logic.
- User-scoped executable worktree extensions remain a separate trust boundary from repository-scoped preparation intent.
- Preparation does not guarantee that reused artifacts are current or correct for every branch or toolchain change.
- This spec does not define exact file allowlists, detection heuristics, or command syntax.
- Preparation must not copy arbitrary untracked files into fresh worktrees.
- Repository-local arbitrary hooks must not run during worktree creation.

## Rationale
- Fresh isolated worktrees are valuable for session isolation, but empty worktrees impose repetitive setup cost on both operators and automated runtimes.
- Repository-local preparation intent keeps common bootstrap behavior shareable across collaborators without requiring each developer to install custom global plugins.
- Trust boundaries stay clearer when repositories can declare non-executable preparation intent while executable worktree-extension power remains user-scoped.
