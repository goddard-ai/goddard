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
Blocked matrix rows assert zero mutation records for faults that block before
submission; submitted-but-unconfirmed rows record the physical input separately
from the daemon's delivery authorization. This establishes the daemon's
delivery/dispatch contract. It cannot prove that native provider tools are
disabled or that a model follows the delivered instructions.

## Coverage matrix

| Contract | Executable coverage |
| --- | --- |
| Reviewed bytes, restart, mutable source replacement, corrupt snapshot | `plan_contract::tests` approval capture and binding tests |
| Draft, inherited binding, conflicting plan/item, pinned older revision, legacy reconfirmation | `plan_contract::tests` binding and legacy tests |
| Same document path under foreign owner; wrong revision/digest | `plan_acceptance_same_path_foreign_owner_cannot_reuse_revision` |
| Broad shared draft discovery, existing pins, private files/memory, invalid principal, traversal | Shared-plan, pin and normalization suites |
| Private linked document stays private in selected revision response | `plan_acceptance_selected_delivery_does_not_expand_private_links` |
| Exact approved bytes with the brief before execution, stray ack and commentary independence, generation/auth/integrity/persistence/transport/dispatch/turn faults | Existing `plan_delivery_*` gate tests |
| Complete input carries exact bytes, assignment/attempt/binding identity and artifact/workspace locators; in-flight briefs withhold, then drain as follow-ups | `plan_acceptance_execution_input_carries_exact_bytes_identity_and_brief` |
| Revocation between submission and adapter acceptance fences release | `plan_acceptance_revocation_between_submission_and_acceptance_fences_release` |
| Rejected submission, accepted-then-unconfirmed submission, readiness projection failure | 3 rows in `plan_acceptance_dispatch_and_projection_failures_preserve_work` |
| 32 KiB complete-bundle bound, oversized plan blocks unsupported; exact reread after response disposal | 2 rows in `plan_acceptance_size_boundary_and_exact_reread_after_compaction` |
| Artifact symlink substitution; private bytes remain undisclosed | Unix `plan_acceptance_symlink_substitution_never_releases_private_bytes` |
| Stale attempt fenced when retirement wins on another thread | `plan_acceptance_concurrent_retirement_fences_stale_attempt` |
| Implementation attachments withheld, persisted, delivered exactly once | `plan_acceptance_attachments_reach_the_execution_input_exactly_once` |
| Settled input turn counts as acceptance; failed settle holds as unconfirmed | 2 rows in `plan_acceptance_settled_input_turn_also_counts_as_acceptance` |
| Intentional interrupt while acceptance is uncertain holds; post-delivery interrupt is ordinary cancellation | 2 rows in `plan_acceptance_intentional_interrupt_mid_submission_holds_uncertain_input` |
| Codex/Devin × direct/worktree-path/cross-project × success/unconfirmed delivery/launch failure/auth revoked during launch/provider switch/workspace switch | 36 rows in `plan_acceptance_provider_workspace_and_launch_fault_matrix` |
| Ordinary no-plan Codex/Devin, employee/unmanaged | 4 rows in `plan_acceptance_ordinary_no_plan_work_keeps_unstaged_dispatch` |
| Real daemon restart redelivers recovered briefs, legacy reviewing-phase records recover without fresh approval, grant retirement, provider event pump | `plan_delivery_runtime_delivers_before_execution_and_recovers_exact_revision` |
| Unsupported adapters and guest modes before provider start | `plan_delivery_preflight_declares_supported_adapters_and_rejects_guest_modes` |
| Durable blocker, stale/forged provenance, replacement attempt, unresolved blocker cannot become ready | Existing controller/runtime blocker tests |
| Pending notice reopens; repeated notification ack remains one logical notice | `plan_acceptance_pending_notice_reopens_and_ack_replay_is_idempotent` |
| Supervisor transport delivers one notice despite revoked employee CLI credential | `plan_blocker_receipt_replays_once_without_employee_cli_authentication` |
| Prerequisite admission before/after completion, separate from binding validation | `plan_acceptance_prerequisite_wait_does_not_change_approved_binding`; prerequisite suite; queued inherited-binding test |

Notification evidence distinguishes a stored notice without acknowledgement from
one with `supervisor_notified_at`. The daemon replay test also counts exactly one
supervisor queued/transcript message. None of these checks means a human read the
notice or validates rendered UI visibility.

The verified run on macOS after the verified-delivery contract reported 48 Rust
tests passed, zero failed and zero ignored, including 36 required-plan runtime
matrix rows and four ordinary work rows. These totals include the existing A/B/C
tests.

## Runtime support and exclusions

Codex and Devin local execution are accepted by preflight. The six matrix
combinations exercise that preflight with real controller persistence and a
scripted external driver. Each launch asserts its assigned working directory;
a project-relative lookalike plan cannot replace the approved delivery. Worktree rows use materialized workspace paths; they do
not create/adopt Git worktrees through the summon operation. Cross-project rows
use another registered execution project under the same daemon. They do not
establish a general filesystem sandbox.

Guest/remapped and cloud required-plan execution are currently unsupported and
must block. Claude and other adapters lack the verified complete-input delivery
contract. The harness verifies rejection; it does not enable these combinations.

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
  and post-submission revocation probes cover deterministic winning interleavings
  only.
- Actual provider context compaction; response disposal plus artifact reread tests
  the durable surface without invoking a provider's compaction engine.
- Persisted `outcomeWait` reevaluation through the daemon scheduler, keyboard/focus
  and UI visibility, and canary timing/rollback. These are not established by
  binding/prerequisite unit checks and are outside D's implementation scope.

An acceptance pass must be reported together with these exclusions. In particular,
a missing attachment contract or unsupported runtime must not be described as
complete coverage of the frozen plan.
