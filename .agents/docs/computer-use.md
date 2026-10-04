# Computer Use

Goddard embeds **Cua Driver 0.28.0** through its native SDK ABI (1.1). The
JavaScript REPL exposes every native tool directly, such as `cua.list_apps()`,
`cua.get_window_state(args)`, and `cua.click(args)`. Setup binds the native methods internally. The bundled skill contains the
host-specific method signatures and direct-call examples; agent code does
not discover or dispatch tools through a catalog API.
Use `jsRepl.write(value)` for output and `await jsRepl.emitImage(image)` for
images. Tool schemas, capture, accessibility, input, and authorization come
from Cua.
The previous `sky` API and custom macOS action engine have been removed.
Computer Use is gated by the **Computer Use** experiment on Settings →
Experiments. The opt-in defaults on in development builds (`bun run dev`) and
is off in release builds until the user enables it. While the experiment is
off the Computer Use settings page stays hidden, the daemon refuses to probe
helper permissions, and `spawn_runtime` clamps `computer_use_enabled` before
any driver starts — so nothing registers the `cua` bridge, attaches the
skill, or reaches the SDK. Turning the experiment off also turns the
Computer Use enable flag itself off.

## Agent interface

All supported providers use the same task-scoped CLI:

```sh
goddard-agent computer js '{"code":"jsRepl.write(1 + 1)","timeout_ms":10000,"title":"Check runtime"}'
goddard-agent computer reset
```

For multiline scripts, `computer js --stdin` reads the JSON object from stdin.
Shared-service providers receive a session-specific launcher path in their
Goddard instructions; use that path in place of `goddard-agent`.

Use `computer run` for a bounded browser workflow when the caller supplies an
explicit URL and goal:

```sh
goddard-agent computer run '{"url":"https://example.com","goal":"Confirm the visible Example Domain heading","verify":{"textContains":["Example Domain"]}}'
```

The optional `values` object maps accessible field labels to supplied text;
`verify` accepts `urlContains`, `textContains`, and exact `fields` checks. The
daemon starts a new isolated browser profile, asks Jev to select among bounded
scrolls, clicks, and supplied text entry built from each fresh semantic
snapshot. It attempts to close that browser
session when the run ends and reports incomplete cleanup if it cannot. It does
not attach to an existing browser profile. Jev receives
redacted page context and candidate ids, never executable refs or supplied
values. The command returns `verified` only when all declared checks pass;
without checks, `done` returns `needs_parent` for parent-agent judgment.
Missing or ambiguous values return `needs_input`. Other outcomes include
`not_verified`, `needs_parent`, `unavailable`, and `stopped`. The action budget
defaults to 12 decisions (maximum 32), and the total time budget defaults to
60 seconds (maximum 120 seconds).

Each task runtime owns a persistent QuickJS kernel. Bindings survive CLI calls
and turns until reset or teardown. Requests to one kernel execute in order on
a dedicated queue; approval waits leave daemon control requests available.
A full queue refuses additional calls before execution, with a message to wait
for the current call. The CLI cannot choose another task's kernel, and it does
not enable cross-task or settings writes when their feature flags are off.

Results preserve text, `isError`, and metadata. Emitted image blocks return
`path` and `mimeType`; the agent opens the path with its image-reading tool.
Screenshots use the daemon's blob store and are referenced by the transcript,
so runtime teardown does not delete retained screenshots. Remote tasks use
paths on their daemon host, where the provider runs. Cloud and sandbox tasks
remain unsupported for Computer Use.

Execution defaults to five minutes, including approval waits; `timeout_ms`
accepts 1–300000. JavaScript failures return an error result and a nonzero CLI
exit status. An action is never automatically replayed after a lost response,
since it may already have run. A transport failure discards the kernel; the
next explicit call starts a fresh one.

Goddard no longer registers CUA MCP tools or a Pi CUA extension with providers.
The native helper's private protocol remains unchanged, as do unrelated MCP
integrations.

## Processes and lifetime

On macOS, the signed `Goddard Computer Use.app` hosts the SDK library directly.
Its Launch Services bridge preserves the helper's existing independent TCC
identity and Goddard's Screen Recording/Accessibility onboarding. The bundled
library is signed with the same identity as the helper. Permission requests
remain host-owned: direct SDK permission checks do not open macOS prompts.

On Windows and Linux, `goddard_computer_use` loads the packaged SDK library into
its own process. It communicates with the REPL over inherited stdin/stdout.
Windows also packages Cua's UIA support executable. No Cua daemon, installation,
Python runtime, Node runtime, or separately running service is required.

Each helper connection owns one SDK runtime. Native tool refusals preserve the
connection and the full result, including error codes, snapshot tokens, capture
metadata, and action outcomes. REPL reset and disconnect close the connection;
Stop cancels native work and ends the host. Interrupted actions are never
automatically retried — the one exception is a request whose newline frame
never reached the helper (a dead helper's write fails before delivery), which
the kernel resends once on a fresh helper since it cannot have run. The
`bring_to_front` tool is omitted from the exposed API. Other tool arguments
pass through to Cua unchanged. All SDK and IPC work occurs outside the GUI
process.

Task approvals map to Cua launch grants: a `browser_prepare` call with
`strategy.kind=existing_profile` is gated under the `browser:existing-profile`
approval scope (never an "always" app grant), and an approved scope adds
`--grant existing-profile` to the next helper spawn — argv on the portable
host, forwarded `--args` to the Launch Services `mcp-child` on macOS, then
`launch_grants` inside `waku_cua_driver_create_v1`'s options JSON, which calls
`configure_launch_grants` before the runtime exists. A helper that predates the
approval is replaced on the next call; the SDK's own R2 gate stays enforced
either way.

Goddard's preview decodes each PNG from the agent's `get_window_state` result on
a background worker. The previous decoded frame stays visible until the latest
replacement is ready; stale or invalid frames are discarded. There is no
second capture or continuous accessibility walk to change the agent's snapshot.

The helper initializes Cua's native cursor facility. On macOS, Cua's renderer
owns the helper's OS main thread while MCP and actions run on workers. Windows
and Linux use Cua's native overlay thread. Cursor movement, action animations,
themes, and reduced-motion handling use the same implementation as standalone
Cua Driver. Headless hosts still report unavailable graphics facilities.

Every executable-relative resource the agent surface and Computer Use resolve —
`goddard-agent`, `goddard_js_repl`, the helper bundle (or the flat helper plus
its SDK library), and the skills tree — is also staged into a daemon-owned
`Runtime/` directory under the per-build application-support root (for example
`~/Library/Application Support/Goddard Debug/Runtime`). Resolvers refresh the
staged copy whenever the packaged source is readable and fall back to it when
the executable's directory was replaced or deleted under the running daemon —
a collected build cache, a rebuilt target, a swapped app bundle. A startup
pass stages all of it eagerly; resolvers also stage lazily per call, so only
daemon boot to first resolve is unprotected. A session whose launch
environment still cannot be minted carries an
`errors.agent_surface_unavailable` transcript notice instead of failing
silently.

## OpenCode 2

OpenCode 2 uses the existing shared service. Goddard registers one temporary MCP
connection per workspace through `/api/mcp` and attaches a session instruction
pointing to the bundled skill (OpenCode limits each entry to 8 KB). `js` and `js_reset` remain direct tools, with
OpenCode's additional codemode wrapper disabled for this server.

OpenCode's `_meta.sessionID` selects a Goddard-owned registration, so each task
has independent JavaScript bindings, native helper processes, cancellation,
and PiP frames. Unregistered sessions cannot execute calls through the bridge.
Detaching a task revokes its registration and removes its instructions; the
last task removes the temporary MCP server. Reconnecting checks the live
server before replacing it, preserving kernels across ordinary SSE reconnects.
No OpenCode configuration files or service descriptors are written.

## Platform requirements

- **macOS:** grant the Goddard helper Screen Recording and Accessibility access
  in Settings > Computer Use. Relaunch the permission-owning helper after a
  grant changes; new REPL connections launch a fresh helper.
- **Windows:** run within the user's interactive desktop. Elevated apps and
  secure desktops remain subject to Windows restrictions. The SDK's native
  capability/error results describe supported input routes.
- **Linux:** X11 uses the active display and AT-SPI accessibility services.
  In a Wayland session, Goddard enables Cua's experimental native Wayland backend
  unless `CUA_DRIVER_RS_ENABLE_WAYLAND` is already set. Window targeting and
  input depend on the compositor's supported routes and installed desktop
  integrations. Cua's GNOME helper files ship under
  `share/goddard/computer-use/wayland-helper`; Goddard does not automatically install
  shell extensions or compositor plugins. `check_permissions` and the native
  tool catalog describe what is available. Unsupported background delivery
  remains an explicit refusal.

Native window IDs are preserved as 64-bit values, including in preview events.
Use IDs and element tokens from fresh observations rather than reconstructing
them or assuming discovery order implies focus.

## Packaging and checks

`scripts/cua-driver.ts` pins release support artifacts and SHA-256 checksums
for macOS, Windows, and Linux on x64 and ARM64. `scripts/cua-host.ts` builds the
SDK from the same pinned source revision with its native host entrypoints
exposed through `resources/computer-use/cua-host.rs`. This small ABI extension
enables Cua's existing cursor facility and main loop; it does not implement
input, capture, or rendering. Authorization still uses Cua's original checks.

The SDK uses its own pinned Rust toolchain and lockfile, isolated from Goddard's
workspace. Sources and builds are cached in the shared build cache
(`scripts/cache-dir.ts`) under `cua-host/`, so normal dev rebuilds — across
worktrees too — reuse the compiled SDK. The macOS bundle, Windows installer/zip,
Linux tarball, and dev watcher package the same host-enabled SDK. `scripts/cua-api.ts` reads the native tool
metadata during packaging and writes the complete API reference into the
bundled skill. Bump the version, all platform checksums, and
the ABI bindings together. Include `resources/computer-use/CUA-LICENSE`.

The portable host can run `list-tools` as a diagnostic without capturing or
operating the desktop. The protocol smoke test uses only tool discovery,
configuration reads, a request missing required arguments, a synthetic image,
and private REPL reset/reconnect:

```sh
cargo build -p waku --bin goddard_js_repl -p waku-computer-use --bin goddard_computer_use
bun scripts/cua-driver.ts bundle target/debug target/debug/resources debug
bun scripts/test-computer-use.ts
```

To test the signed macOS host, pass its packaged REPL and helper executable
paths to `scripts/test-computer-use.ts`. Add `--expect-cursor` to check native
cursor availability in a graphical session without moving or clicking anything.
The CI matrix runs the portable SDK
smoke test on all three operating systems. UI automation tests are separate
and should only run when requested.

References: [in-process SDK guide](https://cua.ai/docs/how-to-guides/driver/use-sdk-in-process),
[native ABI](https://github.com/trycua/cua/blob/1b50c02e2d34734f64d2d22f54eb76cc97b4a663/libs/cua-driver/rust/include/cua_driver_abi.h),
[pinned release](https://github.com/trycua/cua/releases/tag/cua-driver-rs-v0.28.0).
