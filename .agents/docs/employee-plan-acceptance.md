# Required-plan deterministic acceptance checks

Run this harness before evaluating the approved-plan delivery contract. It uses
synthetic documents, identities, credentials and task workspaces, with scripted
providers instead of model processes. It combines existing A/B/C contract tests
with D's fault and runtime matrices. It does not finish rollout verification.

## Run and interpret

From the repository root:

```sh
goddard-agent resource run --json '{"resources":{"native_builds":1},"purpose":"required-plan acceptance"}' -- bun scripts/test-plan-acceptance.ts
```

The command fails on any failed suite or empty filter. Rust reports individual
test results; the final JSON reports aggregate test counts. Matrix rows are
scenario counts within tests, not additional Rust tests. No model, VM, screenshot,
remote provider setup or private incident record is needed. Run on a Unix host:
symlink tests are platform-specific, and the runner fails rather than silently
passing an empty platform filter.

The scripted provider records launch requests and delivered prompts, and writes
an actual task-result file when it receives the synthetic implementation command.
Blocked matrix rows assert both zero mutation records and absence of that file.
This establishes the daemon's review/dispatch contract. It cannot prove that
native provider tools are disabled or that a model follows review instructions.

## Coverage matrix

| Contract | Executable coverage |
| --- | --- |
| Reviewed bytes, restart, mutable source replacement, corrupt snapshot | `plan_contract::tests` approval capture and binding tests |
| Draft, inherited binding, conflicting plan/item, pinned older revision, legacy reconfirmation | `plan_contract::tests` binding and legacy tests |
| Same document path under foreign owner; wrong revision/digest | `plan_acceptance_same_path_foreign_owner_cannot_reuse_revision` |
| Broad shared draft discovery, existing pins, private files/memory, invalid principal, traversal | Shared-plan, pin and normalization suites |
| Private linked document stays private in selected revision response | `plan_acceptance_selected_delivery_does_not_expand_private_links` |
| Read before implementation, missing/partial/stale ack, authentication, artifact/persistence/transport faults | Existing `plan_delivery_*` gate tests |
| Every acknowledgement identity field; repeated/reversed sections, extra response text, response overflow | 12 rows in `plan_acceptance_every_ack_identity_and_section_is_required` |
| Revocation after durable ack, before implementation dispatch | `plan_acceptance_revocation_after_durable_ack_before_dispatch` |
| Review dispatch, implementation dispatch, readiness projection failure | 3 rows in `plan_acceptance_dispatch_and_projection_failures_preserve_work` |
| Inline threshold, sectioned plan, 256 KiB limit, oversized artifact; exact reread after response disposal | 4 rows in `plan_acceptance_size_boundary_and_exact_reread_after_compaction` |
| Artifact symlink substitution; private bytes remain undisclosed | Unix `plan_acceptance_symlink_substitution_never_releases_private_bytes` |
| Old acknowledgement after retirement on another thread | `plan_acceptance_concurrent_retirement_fences_old_acknowledgement` |
| Implementation attachments withheld, persisted, released after ack | `plan_acceptance_withheld_attachments_only_reach_implementation` |
| Codex/Devin × direct/worktree-path/cross-project × success/missing ack/launch failure/auth revoked during launch/provider switch/workspace switch | 36 rows in `plan_acceptance_provider_workspace_and_launch_fault_matrix` |
| Ordinary no-plan Codex/Devin, employee/unmanaged | 4 rows in `plan_acceptance_ordinary_no_plan_work_keeps_unstaged_dispatch` |
| Real daemon restart/review recovery, grant retirement, provider event pump | `plan_delivery_runtime_reviews_before_execution_and_recovers_exact_revision` |
| Unsupported adapters and guest modes before provider start | `plan_delivery_preflight_declares_supported_adapters_and_rejects_guest_modes` |
| Durable blocker, stale/forged provenance, replacement attempt, unresolved blocker cannot become ready | Existing controller/runtime blocker tests |
| Pending notice reopens; repeated notification ack remains one logical notice | `plan_acceptance_pending_notice_reopens_and_ack_replay_is_idempotent` |
| Supervisor transport delivers one notice despite revoked employee CLI credential | `plan_blocker_receipt_replays_once_without_employee_cli_authentication` |
| Prerequisite admission before/after completion, separate from binding validation | `plan_acceptance_prerequisite_wait_does_not_change_approved_binding`; prerequisite suite; queued inherited-binding test |

Notification evidence distinguishes a stored notice without acknowledgement from
one with `supervisor_notified_at`. The daemon replay test also counts exactly one
supervisor queued/transcript message. None of these checks means a human read the
notice or validates rendered UI visibility.

The initial verified run on macOS reported 45 Rust tests passed, zero failed and
zero ignored, including 36 required-plan runtime matrix rows and four ordinary
work rows. These totals include the existing A/B/C tests.

## Runtime support and exclusions

Codex and Devin local execution are accepted by preflight. The six matrix
combinations exercise that preflight with real controller persistence and a
scripted external driver. Each launch asserts its assigned working directory;
a project-relative lookalike plan cannot replace the approved delivery. Worktree rows use materialized workspace paths; they do
not create/adopt Git worktrees through the summon operation. Cross-project rows
use another registered execution project under the same daemon. They do not
establish a general filesystem sandbox.

Guest/remapped and cloud required-plan execution are currently unsupported and
must block. Claude and other adapters lack the supported staged-review contract.
The harness verifies rejection; it does not enable these combinations.

Remaining release evidence:

- Real Codex/Devin adapter end-to-end checks, worktree creation/adoption and moved
  checkout recovery, guest CLI absence and remapping.
- Essential immutable attachment identities and changed/missing/revoked reference
  blockers. Current revision delivery captures document text; links are not an
  attachment contract. The private-link test proves no expansion only.
- Full daemon replacement or process-kill crash injection at each persistence and
  notification boundary. Reopening stores and launch/dispatch fault hooks cover
  narrower deterministic boundaries.
- Overlapping rebinding/withdrawal during in-flight execution. Ordered retirement
  and post-ack revocation probes cover deterministic winning interleavings only.
- Actual provider context compaction; response disposal plus artifact reread tests
  the durable surface without invoking a provider's compaction engine.
- Persisted `outcomeWait` reevaluation through the daemon scheduler, keyboard/focus
  and UI visibility, and canary timing/rollback. These are not established by
  binding/prerequisite unit checks and are outside D's implementation scope.

An acceptance pass must be reported together with these exclusions. In particular,
a missing attachment contract or unsupported runtime must not be described as
complete coverage of the frozen plan.
