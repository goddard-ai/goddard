# Bridge, Security, and Dialogs

Use this reference for detailed JavaScript-to-Zig bridge behavior, built-in window/dialog policies, permission lists, navigation policy, external links, CSP, and dialog field shapes. Keep the common bridge and policy snippets in `SKILL.md`.

## App Bridge

The bridge connects JavaScript to Zig through JSON messages:

```js
const result = await window.zero.invoke("native.ping", { source: "webview" });
```

Handler shape:

```zig
fn ping(context: *anyopaque, invocation: zero_native.bridge.Invocation, output: []u8) anyerror![]const u8 {
    _ = invocation;
    const self: *App = @ptrCast(@alignCast(context));
    self.ping_count += 1;
    return std.fmt.bufPrint(output, "{{\"message\":\"pong\",\"count\":{d}}}", .{self.ping_count});
}
```

Return valid JSON values only. Escape user strings with:

```zig
return zero_native.bridge.writeJsonStringValue(output, user_supplied_name);
```

Wire handlers:

```zig
fn bridge(self: *App) zero_native.BridgeDispatcher {
    self.handlers = .{.{ .name = "native.ping", .context = self, .invoke_fn = ping }};
    return .{
        .policy = .{ .enabled = true, .commands = &policies },
        .registry = .{ .handlers = &self.handlers },
    };
}
```

Bridge invocation data:

| Field | Description |
| --- | --- |
| `request.id` | Caller ID, max 64 bytes |
| `request.command` | Command name, max 128 bytes, no `/` or spaces |
| `request.payload` | JSON payload string |
| `source.origin` | Requesting page origin |
| `source.window_id` | Requesting window |

Limits:

| Limit | Value |
| --- | --- |
| Message | 16 KiB |
| Response | 16 KiB |
| Result | 12 KiB |
| ID | 64 bytes |
| Command | 128 bytes |

Error codes: `invalid_request`, `unknown_command`, `permission_denied`, `handler_failed`, `payload_too_large`, `internal_error`.

## Built-in Commands

Built-ins are controlled by `builtin_bridge`, separate from app commands.

Window commands:

| Command | Permission | Description |
| --- | --- | --- |
| `zero-native.window.list` | `window` | List windows |
| `zero-native.window.create` | `window` | Create a window |
| `zero-native.window.focus` | `window` | Focus a window |
| `zero-native.window.close` | `window` | Close a window |

Dialog commands are always default-deny and require explicit policy:

| Command | Description |
| --- | --- |
| `zero-native.dialog.openFile` | Open file dialog |
| `zero-native.dialog.saveFile` | Save file dialog |
| `zero-native.dialog.showMessage` | Message dialog |

Example policy:

```zig
.builtin_bridge = .{
    .enabled = true,
    .commands = &.{
        .{ .name = "zero-native.window.create", .permissions = .{ "window" }, .origins = .{ "zero://app" } },
        .{ .name = "zero-native.dialog.openFile", .origins = .{ "zero://app" } },
        .{ .name = "zero-native.dialog.showMessage", .origins = .{ "zero://app" } },
    },
},
```

## Permissions and Capabilities

Use the smallest permission set that covers the app.

Permissions:

| Permission | Grants |
| --- | --- |
| `window` | Window create/focus/close |
| `filesystem` | File system access from bridge commands |
| `clipboard` | Clipboard read/write |
| `network` | Native network requests |
| `camera` | Camera access |
| `microphone` | Microphone access |
| `location` | Location services |
| `notifications` | System notifications |

Capabilities: `webview`, `js_bridge`, `native_module`, `filesystem`, `network`, `clipboard`.

Custom permissions should use reverse-DNS names.

## Navigation and External Links

Main-frame navigation is allowlisted:

```zig
.security = .{
    .navigation = .{
        .allowed_origins = .{
            "zero://app",
            "zero://inline",
            "http://127.0.0.1:5173",
        },
    },
},
```

Prefer exact origins over `"*"`. Use broad patterns only in local development or for commands that expose no native state.

External links are denied by default. To open specific links in the system browser:

```zig
.security = .{
    .navigation = .{
        .external_links = .{
            .action = "open_system_browser",
            .allowed_urls = .{ "https://example.com/docs/*" },
        },
    },
},
```

## CSP

For packaged assets:

```html
<meta http-equiv="Content-Security-Policy"
  content="default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'">
```

For inline examples only, add minimal inline allowances:

```html
<meta http-equiv="Content-Security-Policy"
  content="default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'">
```

For dev servers, extend `connect-src` only to the required local dev and WebSocket origins.

## Dialog Shapes

Open file dialog fields:

| Field | Type | Default |
| --- | --- | --- |
| `title` | `[]const u8` | `""` |
| `default_path` | `[]const u8` | `""` |
| `filters` | `[]const FileFilter` | `&.` |
| `allow_directories` | `bool` | `false` |
| `allow_multiple` | `bool` | `false` |

Save dialog fields: `title`, `default_path`, `default_name`, `filters`.

Message dialog fields: `style` (`info`, `warning`, `critical`), `title`, `message`, `informative_text`, `primary_button`, `secondary_button`, `tertiary_button`.

JavaScript JSON uses camelCase:

```js
const files = await window.zero.invoke("zero-native.dialog.openFile", {
  title: "Select a file",
  defaultPath: "/home",
  allowMultiple: true,
  allowDirectories: false,
});
```
