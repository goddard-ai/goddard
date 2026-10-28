# Computer Use stall investigation

Source task: `e3b5b0bc-a74a-4127-af61-e814a0edfc98`, reported on macOS on
2026-10-28. This report contains no external account or page contents. No
external account interaction, screenshots, or visual analysis were used.

## Findings

| Area | Finding | Disposition |
| --- | --- | --- |
| Queue timeout | `Service::call_inner` took an unbounded connection mutex before starting its response wait. It then allowed an additional 10 seconds beyond the JS budget. | Fixed: one service deadline covers lock admission, pipe writing, and response waiting. The kernel receives only the remaining budget. |
| Stop | Ordinary JS calls did not capture a cancellation generation. Only bounded browser runs supplied one, and that generation was checked after acquiring the lock. | Fixed: all calls capture a generation and poll it while queued, writing, or awaiting a response. Cancelled queued actions are rejected without execution. |
| Reset | Reset used the same execution lock and could wait behind a stuck call for its entire timeout. | Fixed: reset invalidates active/queued calls first. An unresponsive active transport is discarded without replay, then reset proceeds with a 10-second service budget. |
| CLI timeout | `computer` passed no timeout to the daemon request. An unhealthy runtime/daemon could keep the shell and resource supervisor waiting indefinitely. | Fixed: bounded request wait, allowing 2 seconds beyond the service budget for delivery. Browser runs additionally retain their existing 10-second cleanup allowance. Connection establishment has its own transport behavior. |
| Nested errors | `jsRepl.write` successfully printing a native `isError:true` value is a successful JavaScript execution. CLI exit status follows the outer JS result. | Intentional composition behavior; documented how to throw explicitly on a native error. |
| Existing profile | `NativeComputerUseClient::authorize` requires the task-scoped `browser:existing-profile` approval and an SDK launch grant. App/browser approval alone does not authorize it. | Expected gate, preserved. Evidence does not establish whether a prompt was pending in the source task or who/what stopped the operation. |
| Popup classification | A cached SDK revision uses Chromium ownership + web-area ancestry + `AXPopUpButton` to classify a web select. It does not inspect the HTML tag. | Overbroad classification found in that source; actual source-task control and loaded SDK revision remain unverified. No runtime popup-routing change was made. |
| Broker | Acquire and cancel use the authenticated agent principal. Cancel looks up reservation ID plus task, and uses the same error for absent and foreign IDs. | No identity defect established. Task UUID and reservation UUID are distinct. |

## Relevant paths

- `crates/waku-drivers/src/driver/computer_use/cli.rs`:
  `Service::call`, `call_inner`, `cancel`, and `shutdown` own serialization,
  cancellation generations, process loss, and bounded transport waiting.
- `crates/waku-drivers/src/driver/computer_use.rs`:
  `ComputerUseRuntime::stop` increments the service generation and
  `stop_registered_processes` writes `cancel-kernel` and stops registered helpers.
- `src/js_repl.rs`: `JavaScriptRepl::execute` clears a prior cancellation marker
  on entry; `NativeComputerUseClient::authorize` polls stop/disable/deadline
  while waiting for approval; `RequestWatchdog` enforces native-call deadlines.
- `crates/waku-daemon/src/daemon/queries.rs`: `agent_computer_use` checks task
  identity, global settings, and task availability before calling the service.
- `crates/waku-agent/src/main.rs`: `computer` bounds the wire request and maps
  outer `isError` to a nonzero exit status.
- `crates/waku-agent/src/resources.rs`: `run` waits for its registered child,
  checks cancellation, and releases its reservation after completion.
- `crates/waku-base/src/resource_broker.rs`: `owned` fences operations by
  reservation ID and task; released capacity stays reserved while registered
  process-group work remains alive.

## Popup and API version limit

This checkout pins Cua SDK `0.28.0` at
`1b50c02e2d34734f64d2d22f54eb76cc97b4a663` in `scripts/cua-host.ts`.
Its source lacks `select_menu_would_raise_window`. The exact refusal code is
present in another locally cached SDK source revision,
`85e2b1318bd77ccf3cfa88ccdf875a4bb7eaed19`:

- `libs/cua-driver/rust/crates/platform-macos/src/tools/set_value.rs`:
  `is_chromium_web_select` checks role, Chromium ownership, and web ancestry.
- `.../tools/click.rs`: background popup routing consults that classifier.
- `.../tools/set_value.rs`: `chromium_select_open_refusal` constructs the refusal.

A custom HTML button with `aria-haspopup="menu"` and a native HTML `select`
can both present popup accessibility semantics; role alone cannot safely choose
between opening an action menu and selecting an option. The existing refusal
protects against native menus raising the browser. Removing that guard based
on an unknown/custom AX role would weaken the permission boundary.

The focused SDK follow-up is to distinguish a verified native select from a
custom menu trigger using trustworthy DOM/AX metadata, retaining a conservative
refusal when ambiguous. Verify on a local HTML fixture containing both controls,
including an obscured browser window and an unavailable DOM attachment. Do not
use the external account to test it. Resolve the shipped SDK version before
applying an upstream patch; modifying a cache would not produce a durable fix
in this checkout.

Likewise, this checkout's generated `get_window_state` API does not declare
`tree_format`. The reported default/`both` behavior cannot be validated against
this older API. Do not hand-edit its generated declaration to invent support.

## Recovery

1. Check `goddard-agent resource status` and local process ownership before
   cancelling anything. During this investigation there were no listed
   reservations and no matching `goddard-agent`, `js-repl`, `cua-host`, or
   `computer-use` processes. Nothing from the source task was killed.
2. In a build containing this fix, run `goddard-agent computer reset` from the
   affected task. It cancels earlier queued work; it does not override disabled
   Computer Use, denied access, or task boundaries. Bootstrap `cua` again after
   successful reset. This investigation did not reset the source task.
3. Start with a bounded pure JS probe:
   `goddard-agent computer js --timeout-ms 1000 --text 'jsRepl.write(1)'`.
   Native reads may still require their normal approval. For a current API that
   supports it, request AX-only state with `include_screenshot:false` and
   `tree_format:"both"` when element arrays are needed.
4. After any lost response, inspect state before deciding whether to repeat an
   action: it may already have executed. Never automatically replay mutations.
5. For broker cancellation, use the reservation's `id` from status, not its
   `task` UUID, under the owning task credential. Preserve ownership checks and
   wait for registered work to exit before expecting capacity to be reusable.

## Verification scope

Regression coverage uses a Python kernel fixture and a local mock daemon;
no browser or account access is required. It exercises queue expiry without
execution, stop of an unresponsive kernel, reset with stale queued actions,
a blocked stdin pipe, transport disconnect, retained diagnostics, and CLI
termination when a healthy connection never returns an RPC response. Existing
resource CLI tests cover exit status, release, nested borrowing, registration
failure, and cancellation of the owned process group.

These tests establish the local failure paths, not the exact historical cause
of the source-task stall. The signed-in browser approval UI, actual external
control, and rebuilt-app provider interaction were not exercised.

Completed checks:

- `mbx test -p waku-drivers --lib computer_use::cli`: 9 passed, 2
  packaged-engine tests ignored (require `GODDARD_TEST_JS_REPL`).
- `mbx test -p waku-agent --test connection_close --test resources`: 6 passed.
- `bun ./scripts/changelog.ts check` and `git diff --check`: passed.

Native compilation/tests ran under `native_builds:1` resource reservations,
which completed and released. No debug app was relaunched or native browser
fixture exercised; this remains a validation gap for actual popup routing.
