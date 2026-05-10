---
name: zero-native
description: Build, debug, test, and package zero-native Zig desktop apps using WebView or Chromium/CEF. Use for manifests, frontend sources, bridge commands, security policy, windows, dialogs, automation, and distribution.
---

# zero-native

Use this skill to make zero-native changes without re-deriving its common Zig APIs, manifest policy, frontend source switching, bridge limits, or runtime wiring.

## Default Stance

- Keep the native layer small and explicit: app state, runtime wiring, bridge handlers, platform services, and policies belong in Zig.
- Treat WebView content as untrusted. Grant native power only through exact origins, narrow permissions, and explicit bridge command policies.
- Prefer the system WebView for small apps and development simplicity; choose Chromium/CEF only when consistent Chromium rendering is a requirement.
- Use generated project conventions before inventing structure: `app.zon`, `src/main.zig`, `src/runner.zig`, `frontend/`, and `zig build` steps.
- For frontend frameworks, use `frontend.sourceFromEnv` or `zero-native dev` so development loads a local server and production loads bundled assets.
- Validate manifests and packaging paths with `zero-native validate`, `zero-native doctor --manifest app.zon --strict`, and relevant `zig build` test/package steps.

## Start Here

1. Identify whether the task is scaffold, app/runtime code, frontend integration, bridge/security, web engine choice, packaging, or testing/debugging.
2. Inspect the local project files before editing:
   - `app.zon` for app metadata, permissions, navigation, bridge commands, windows, web engine, CEF, and frontend config.
   - `src/main.zig` for the app struct, `app()` source selection, bridge dispatcher, and lifecycle callbacks.
   - `src/runner.zig` for platform selection, runtime options, trace/logging, state restore, built-in bridge policy, and automation wiring.
   - `build.zig` for build options such as `platform`, `web-engine`, `automation`, `js-bridge`, `debug-overlay`, and packaging steps.
3. Read one focused reference only when the task needs detail:
   - [setup-scaffolding-cli.md](./references/setup-scaffolding-cli.md): install, prerequisites, `zero-native init`, generated project layout, first run, CLI flags, environment variables.
   - [manifest-and-runtime.md](./references/manifest-and-runtime.md): full `app.zon` fields, runner details, runtime methods, window persistence, extensions, embedded apps.
   - [frontend-and-dev-server.md](./references/frontend-and-dev-server.md): frontend dev server setup, framework recipes, bundled assets, production source variants.
   - [bridge-security-dialogs.md](./references/bridge-security-dialogs.md): detailed bridge policy, built-in commands, permissions, navigation policy, CSP, dialog shapes.
   - [web-engines-and-packaging.md](./references/web-engines-and-packaging.md): system vs Chromium, CEF setup, packaging, signing, icons, updates.
   - [testing-debugging-automation.md](./references/testing-debugging-automation.md): headless tests, WebView smoke tests, automation CLI, tracing, logs, doctor.

## Common APIs

### App and Sources

Define an app by returning `zero_native.App` from app state:

```zig
fn app(self: *@This()) zero_native.App {
    return .{
        .context = self,
        .name = "hello",
        .source = zero_native.WebViewSource.html(
            \\<!doctype html>
            \\<html><body>Hello from zero-native</body></html>
        ),
    };
}
```

Common `App` fields:

| Field | Use |
| --- | --- |
| `context` | Pointer to app state |
| `name` | Name used in traces and automation |
| `source` | Static initial `WebViewSource` |
| `source_fn` | Dynamic source resolver; overrides `source` |
| `start_fn` | Startup callback after first window load |
| `event_fn` | Lifecycle and command callback |
| `stop_fn` | Shutdown callback |

Use `WebViewSource.html(content)` for inline HTML, `WebViewSource.url(address)` for a URL, and `WebViewSource.assets(.{ .root_path = "dist", .entry = "index.html" })` for bundled files. Packaged assets normally use the `zero://app` origin.

### Frontend Sources

For React, Vue, Svelte, Next.js, or another frontend with a build step, use `source_fn` plus `zero_native.frontend.sourceFromEnv`:

```zig
fn source(context: *anyopaque) anyerror!zero_native.WebViewSource {
    const self: *App = @ptrCast(@alignCast(context));
    return zero_native.frontend.sourceFromEnv(self.env_map, .{
        .dist = "dist",
        .entry = "index.html",
    });
}
```

`sourceFromEnv` reads `ZERO_NATIVE_FRONTEND_URL`: when set, it loads that URL; otherwise it serves assets from `dist`. Common config fields:

| Field | Default | Use |
| --- | --- | --- |
| `dist` | `"dist"` | Built frontend output |
| `entry` | `"index.html"` | Entry file inside `dist` |
| `origin` | `"zero://app"` | Asset origin |
| `spa_fallback` | `true` | Serve `entry` for unknown routes |
| `dev_url_env` | `"ZERO_NATIVE_FRONTEND_URL"` | Dev URL environment variable |

Use `zero_native.frontend.productionSource(.{ .dist = "dist", .entry = "index.html" })` when a packaged build should always use local assets.

### Bridge Commands

Register native Zig handlers for JavaScript calls through `window.zero.invoke`. Handler results must be valid JSON values and fit in the 12 KiB result limit:

```zig
fn ping(context: *anyopaque, invocation: zero_native.bridge.Invocation, output: []u8) anyerror![]const u8 {
    _ = invocation;
    const self: *App = @ptrCast(@alignCast(context));
    self.ping_count += 1;
    return std.fmt.bufPrint(output, "{{\"message\":\"pong\",\"count\":{d}}}", .{self.ping_count});
}
```

Wire a dispatcher and policy:

```zig
fn bridge(self: *App) zero_native.BridgeDispatcher {
    self.handlers = .{.{ .name = "native.ping", .context = self, .invoke_fn = ping }};
    return .{
        .policy = .{ .enabled = true, .commands = &policies },
        .registry = .{ .handlers = &self.handlers },
    };
}
```

Call from JavaScript:

```js
const result = await window.zero.invoke("native.ping", { source: "webview" });
```

Use `zero_native.bridge.writeJsonStringValue(output, value)` for user-controlled strings. Bridge errors surface as `invalid_request`, `unknown_command`, `permission_denied`, `handler_failed`, `payload_too_large`, or `internal_error`.

### Security Policy

Default to exact origin allowlists and narrow permissions:

```zig
.security = .{
    .permissions = &.{ "window" },
    .navigation = .{ .allowed_origins = &.{ "zero://app" } },
},
```

Add dev origins explicitly, such as `http://127.0.0.1:5173`. Prefer exact origins over `"*"`.

Common permissions are `window`, `filesystem`, `clipboard`, `network`, `camera`, `microphone`, `location`, and `notifications`. Custom permissions should use reverse-DNS names.

App bridge commands must be registered in Zig and allowed by policy before they run. Built-in dialogs are always default-deny and require explicit `builtin_bridge` entries:

```zig
.builtin_bridge = .{
    .enabled = true,
    .commands = &.{
        .{ .name = "zero-native.dialog.openFile", .origins = .{ "zero://app" } },
        .{ .name = "zero-native.dialog.showMessage", .origins = .{ "zero://app" } },
    },
},
```

### Runtime and Windows

`src/runner.zig` typically creates the runtime and calls `runtime.run(app)`:

```zig
var runtime = zero_native.Runtime.init(.{
    .platform = my_platform,
    .trace_sink = fanout.sink(),
    .bridge = my_app.bridge(),
    .builtin_bridge = builtin_policy,
    .security = security_policy,
    .window_state_store = state_store,
    .automation = automation_server,
});
try runtime.run(my_app.app());
```

Common runtime options: `platform`, `trace_sink`, `log_path`, `extensions`, `bridge`, `builtin_bridge`, `security`, `automation`, `window_state_store`, and `js_window_api`.

Create and manage windows from Zig:

```zig
const info = try runtime.createWindow(.{
    .label = "tools",
    .title = "Tools",
    .default_frame = zero_native.geometry.RectF.init(80, 80, 420, 320),
});
try runtime.focusWindow(info.id);
```

Set `js_window_api = true` only with exact origins and the `window` permission when JavaScript should use `window.zero.windows.create/list/focus/close`.

### Dialogs

Use built-in dialog commands from JavaScript only after enabling their policy:

```js
const files = await window.zero.invoke("zero-native.dialog.openFile", {
  title: "Select a file",
  allowMultiple: true,
});

const result = await window.zero.invoke("zero-native.dialog.showMessage", {
  style: "warning",
  title: "Confirm",
  message: "Delete this item?",
  primaryButton: "Delete",
  secondaryButton: "Cancel",
});
```

Dialog JSON uses camelCase. Zig dialog option fields use snake_case.

## Validation

Run focused validation after changes:

```bash
zero-native validate app.zon
zero-native doctor --manifest app.zon --strict
zig build test
```

Use GUI smoke tests only when a GUI-capable macOS session is available:

```bash
zig build test-webview-smoke -Dplatform=macos
zig build test-webview-cef-smoke -Dplatform=macos -Dweb-engine=chromium
```
