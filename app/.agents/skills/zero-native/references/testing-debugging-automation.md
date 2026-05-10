# Testing, Debugging, and Automation

Use this reference for headless tests, GUI smoke tests, automation, doctor, tracing, logs, panic capture, and debug overlays.

## Test Strategy

Use headless tests for bridge, lifecycle, policies, and platform behavior:

```bash
zig build test
zig build test-desktop
zig build test-platform-info
```

Use `TestHarness` and `NullPlatform` to test without a window server:

```zig
var harness: zero_native.TestHarness = undefined;
harness.init(.{});
```

```zig
var null_platform = zero_native.NullPlatform.init(.{});
var runtime = zero_native.Runtime.init(.{
    .platform = null_platform.platform(),
});
```

Use WebView smoke tests only when a GUI-capable macOS session is available:

```bash
zig build test-webview-smoke -Dplatform=macos
zig build test-webview-cef-smoke -Dplatform=macos -Dweb-engine=chromium
```

CEF smoke tests also require a local CEF layout or `-Dcef-auto-install=true`.

## Automation Server

Enable automation at build time:

```bash
zig build run-webview -Dautomation=true
```

Wire a server into runtime options:

```zig
const server = zero_native.automation.Server.init(io, ".zig-cache/zero-native-automation", "My App");
var runtime = zero_native.Runtime.init(.{
    .platform = my_platform,
    .automation = server,
});
```

Default directory: `.zig-cache/zero-native-automation`.

Published files:

| File | Description |
| --- | --- |
| `snapshot.txt` | Runtime state and `ready=true/false` |
| `accessibility.txt` | Accessibility tree summary |
| `windows.txt` | Window list |
| `screenshot.ppm` | Screenshot placeholder |
| `command.txt` | CLI command input |
| `bridge-response.txt` | Last bridge response |

CLI flow:

```bash
zero-native automate wait
zero-native automate snapshot
zero-native automate screenshot
zero-native automate reload
zero-native automate bridge '{"id":"1","command":"native.ping","payload":{"source":"automation"}}'
```

Smoke-test pattern:

1. Build and start with `-Dautomation=true`.
2. Run `zero-native automate wait`.
3. Run `zero-native automate snapshot` and inspect window/source metadata.
4. Send a bridge request with `zero-native automate bridge`.
5. Verify `bridge-response.txt`.

## Doctor and Validation

`zero-native doctor` checks host platform, WebView availability, manifest validity, log paths, CEF layout, and signing tools.

```bash
zero-native doctor
zero-native doctor --manifest app.zon --strict
zero-native doctor --manifest app.zon --web-engine chromium
```

Flags:

| Flag | Description |
| --- | --- |
| `--strict` | Exit non-zero on any warning |
| `--manifest` | Path to `app.zon` |
| `--web-engine` | Temporarily override manifest engine |
| `--cef-dir` | Temporarily override CEF path |
| `--cef-auto-install` | Allow automatic CEF install for checks |

## Tracing and Logs

Trace modes:

| Mode | Description |
| --- | --- |
| `off` | No traces |
| `events` | Lifecycle/platform events; default |
| `runtime` | Runtime internals |
| `all` | Everything |

Enable at build time or parse in code:

```bash
zig build run-webview -Dtrace=all
```

```zig
const mode = zero_native.debug.parseTraceMode("all");
```

Trace sinks include `FileTraceSink`, `FanoutTraceSink`, and stdout sink from the trace module:

```zig
var file_sink = zero_native.debug.FileTraceSink.init(io, log_dir, log_file, .json_lines);
var sinks = [_]trace.Sink{ stdout_sink.sink(), file_sink.sink() };
var fanout = zero_native.debug.FanoutTraceSink{ .sinks = &sinks };
```

Wire with:

```zig
var runtime = zero_native.Runtime.init(.{
    .platform = my_platform,
    .trace_sink = fanout.sink(),
});
```

Log locations:

| Platform | Path |
| --- | --- |
| macOS | `~/Library/Logs/<bundle-id>/zero-native.jsonl` |
| Linux | `~/.local/state/<bundle-id>/logs/zero-native.jsonl` |
| Windows | `%LOCALAPPDATA%<bundle-id>\Logs\zero-native.jsonl` |

Environment:

| Variable | Description |
| --- | --- |
| `ZERO_NATIVE_LOG_DIR` | Override log output directory |
| `ZERO_NATIVE_LOG_FORMAT` | `text` or `jsonl` |

## Panic Capture and Debug Overlay

Enable panic capture:

```zig
pub const panic = std.debug.FullPanic(zero_native.debug.capturePanic);
zero_native.debug.installPanicCapture(io, log_setup.paths);
```

On panic, zero-native writes `last-panic.txt`, appends a fatal trace record, then invokes Zig's default panic handler.

Enable the WebView debug overlay:

```bash
zig build run-webview -Ddebug-overlay=true
```
