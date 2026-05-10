# Manifest and Runtime

Use this reference for full `app.zon` fields, lifecycle details, runtime methods, window persistence, extensions, embedded runtime work, and less-common platform services. Keep the common app/source/runtime API shapes in `SKILL.md`.

## app.zon

`app.zon` declares app metadata, permissions, bridge policies, security rules, windows, frontend config, and the web engine.

```zig
.{
    .id = "com.example.myapp",
    .name = "myapp",
    .display_name = "My App",
    .version = "1.0.0",
    .icons = .{ "assets/icon.icns", "assets/icon.ico" },
    .platforms = .{ "macos", "linux" },
    .permissions = .{ "window" },
    .capabilities = .{ "webview", "js_bridge" },
    .bridge = .{
        .commands = .{
            .{ .name = "native.ping", .origins = .{ "zero://app" } },
        },
    },
    .security = .{
        .navigation = .{
            .allowed_origins = .{ "zero://app", "http://127.0.0.1:5173" },
            .external_links = .{ .action = "deny" },
        },
    },
    .web_engine = "system",
    .cef = .{ .dir = "third_party/cef/macos", .auto_install = false },
    .windows = .{
        .{ .label = "main", .title = "My App", .width = 720, .height = 480, .restore_state = true },
    },
}
```

Important fields:

| Field | Purpose |
| --- | --- |
| `id` | Reverse-DNS bundle identifier and log/package identity |
| `name` | Short machine name |
| `display_name` | Menu bar and fallback window title |
| `version` | Package metadata |
| `icons` | Platform icons copied into packages |
| `platforms` | Target platforms: `macos`, `linux`, `windows` |
| `permissions` | Runtime grants checked before native commands |
| `capabilities` | Declared app features |
| `bridge` | App-defined bridge command policy |
| `security` | Navigation and external link policy |
| `web_engine` | `system` or `chromium` |
| `cef` | Chromium/CEF directory and auto-install setting |
| `windows` | Initial windows and restore policy |
| `frontend` | Built assets and dev server config |

Validate after manifest edits:

```bash
zero-native validate app.zon
zero-native doctor --manifest app.zon --strict
```

## App

An app provides context, name, source, and optional callbacks. Use the concise API examples in `SKILL.md` for common edits; use this table when checking complete field semantics:

| Field | Description |
| --- | --- |
| `context` | Pointer to app state |
| `name` | App name for traces and automation |
| `source` | Initial `WebViewSource` |
| `source_fn` | Dynamic source resolver, overrides `source` |
| `start_fn` | Called after the runtime starts and first window loads |
| `event_fn` | Called for lifecycle and command events |
| `stop_fn` | Called before shutdown |

Source constructors:

- `.html(content)`: inline HTML served as `zero://inline`.
- `.url(address)`: local or remote URL.
- `.assets(options)`: local asset tree served from an origin such as `zero://app`.

Asset source fields:

| Field | Default | Description |
| --- | --- | --- |
| `root_path` | required | Directory containing frontend assets |
| `entry` | `"index.html"` | Entry file |
| `origin` | `"zero://app"` | Asset origin |
| `spa_fallback` | `true` | Serve entry for unknown routes |

Lifecycle events dispatched through `event_fn` include `start`, `frame`, and `stop`.

## RuntimeOptions

`src/runner.zig` usually selects the platform, sets tracing and panic capture, configures window state persistence, creates the `Runtime`, and calls `runtime.run(app)`.

Common runtime fields:

| Field | Default | Description |
| --- | --- | --- |
| `platform` | required | macOS, Linux, or `NullPlatform` |
| `trace_sink` | `null` | Structured trace destination |
| `log_path` | `null` | Persistent log path |
| `extensions` | `null` | Module registry |
| `bridge` | `null` | App-defined bridge dispatcher |
| `builtin_bridge` | default policy | Built-in windows/dialogs policy |
| `security` | empty policy | Navigation and permissions |
| `automation` | `null` | File-based automation server |
| `window_state_store` | `null` | Persistent geometry/state |
| `js_window_api` | `false` | Expose `window.zero.windows.*` |

Runtime methods include `run`, `createWindow`, `listWindows`, `focusWindow`, `closeWindow`, `invalidate`, `frameDiagnostics`, `dispatchEvent`, `dispatchPlatformEvent`, and `automationSnapshot`.

## Windows

Create secondary windows from Zig:

```zig
const info = try runtime.createWindow(.{
    .label = "tools",
    .title = "Tools",
    .default_frame = zero_native.geometry.RectF.init(80, 80, 420, 320),
});
try runtime.focusWindow(info.id);
```

Expose JS window helpers only with `js_window_api = true`, exact origins, and the `window` permission.

Limits:

| Constant | Value |
| --- | --- |
| `max_windows` | 16 |
| `max_window_label_bytes` | 64 |
| `max_window_title_bytes` | 128 |

Declare windows in `app.zon`:

```zig
.windows = .{
    .{ .label = "main", .title = "My App", .width = 720, .height = 480, .restore_state = true },
},
```

`window_state.Store` persists geometry to `windows.zon` in the app state directory and merges restored windows by `label` or `id`.

## System Tray

Tray support is currently implemented on macOS. Linux returns `UnsupportedService` until a portable status notifier implementation is selected.

`TrayOptions`:

| Field | Type | Default |
| --- | --- | --- |
| `icon_path` | `[]const u8` | `""` |
| `tooltip` | `[]const u8` | `""` |
| `items` | `[]const TrayMenuItem` | `&.` |

`TrayMenuItem`:

| Field | Type | Default |
| --- | --- | --- |
| `id` | `TrayItemId` (`u32`) | `0` |
| `label` | `[]const u8` | `""` |
| `separator` | `bool` | `false` |
| `enabled` | `bool` | `true` |

Platform service methods: `createTray(options)`, `updateTrayMenu(items)`, and `removeTray()`. Tray clicks dispatch a `CommandEvent` named `"tray.action"` through `event_fn`.

## Extensions and Embedded Apps

Use `ModuleRegistry` for modular runtime hooks with `start_fn`, `stop_fn`, and `command_fn`. Validate module IDs before runtime use.

Use `EmbeddedApp` when an existing event loop drives the runtime:

```zig
var embedded = zero_native.embed.EmbeddedApp.init(my_app.app(), my_platform);
try embedded.start();
try embedded.frame();
try embedded.resize(new_surface);
try embedded.stop();
```

Use `NullPlatform` for headless tests or embedded examples.
