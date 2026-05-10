# Setup, Scaffolding, and CLI

Use this reference for installation, prerequisites, creating a new project, first-run workflows, CLI command lookup, and environment variables.

## Install

Install the zero-native CLI from npm:

```bash
npm install -g zero-native
```

Prerequisites:

- Zig 0.16.0+
- Node.js with npm for generated frontends
- macOS or Linux for current desktop development; Windows is in progress

## Create a Project

Scaffold a project:

```bash
zero-native init my_app --frontend next
cd my_app
```

Frontend options: `next`, `vite`, `react`, `svelte`, and `vue`.

Generated project layout:

| File | Purpose |
| --- | --- |
| `build.zig` | Zig build graph with platform, trace, debug-overlay, automation, js-bridge, and web-engine options |
| `build.zig.zon` | Zig package manifest with the zero-native dependency |
| `app.zon` | App metadata, icons, permissions, bridge commands, security policy, windows, frontend config |
| `src/main.zig` | App state with `app()` and optional `bridge()` |
| `src/runner.zig` | Platform wiring, runtime options, tracing, logs, panic capture, state restore |
| `assets/icon.icns` | macOS package icon |
| `frontend/` | Starter frontend for the selected framework |

Run:

```bash
zig build run
```

The first frontend build installs generated frontend dependencies, then opens a native window.

## macOS Beta Path

Use this path when validating a macOS beta app:

```bash
zero-native init my_app --frontend next
cd my_app
zig build run
zero-native cef install
zig build run
zero-native package --target macos --signing identity --identity "Developer ID Application: Your Name"
zero-native doctor --manifest app.zon --strict
```

Before the Chromium run, set:

```zig
.web_engine = "chromium",
.cef = .{ .dir = "third_party/cef/macos", .auto_install = false },
```

Prefer manifest-driven engine settings for normal app workflows. Use `-Dweb-engine` or `--web-engine` for one-off overrides.

## CLI Commands

| Command | Description |
| --- | --- |
| `zero-native init [path] --frontend <next|vite|react|svelte|vue>` | Scaffold a project |
| `zero-native dev --binary <path>` | Run with a managed frontend dev server |
| `zero-native doctor` | Check environment, WebView, manifest, CEF |
| `zero-native cef install` | Install prepared CEF runtime |
| `zero-native cef path` | Print the default or configured CEF directory |
| `zero-native cef doctor` | Check only the CEF layout |
| `zero-native validate [app.zon]` | Validate the manifest |
| `zero-native package` | Package for distribution |
| `zero-native bundle-assets [app.zon] [assets] [output]` | Copy frontend assets |
| `zero-native automate <command>` | Interact with automation server |
| `zero-native version` | Print version |

## `zero-native dev`

```bash
zero-native dev --binary zig-out/bin/MyApp
zero-native dev --binary zig-out/bin/MyApp --url http://127.0.0.1:3000/ --command "npm run dev"
zero-native dev --binary zig-out/bin/MyApp --timeout-ms 60000
```

Flags:

| Flag | Description |
| --- | --- |
| `--binary` | Compiled native binary; required |
| `--manifest` | Path to `app.zon`; default `app.zon` |
| `--url` | Override manifest dev URL |
| `--command` | Override manifest dev command |
| `--timeout-ms` | Override readiness timeout |

## `zero-native cef`

| Flag | Description |
| --- | --- |
| `--dir` | CEF install directory; defaults under `third_party/cef/<platform>` |
| `--version` | Prepared CEF binary version |
| `--source` | `prepared` or `official`; default `prepared` |
| `--download-url` | Override runtime release base URL |
| `--allow-build-tools` | Allow official CEF path to invoke local build tools |
| `--force` | Redownload and replace the target directory |

## `zero-native package`

| Flag | Description |
| --- | --- |
| `--target` | `macos`, `linux`, `windows`, `ios`, or `android` |
| `--manifest` | `app.zon` path |
| `--output` | Package output path |
| `--binary` | Built binary path |
| `--assets` | Frontend assets directory |
| `--optimize` | Optimization level |
| `--web-engine` | Temporary `system` or `chromium` override |
| `--cef-dir` | Temporary CEF directory override |
| `--cef-auto-install` | Allow prepared CEF installation |
| `--signing` | `none`, `adhoc`, or `identity` |
| `--identity` | Code signing identity |
| `--entitlements` | Entitlements file |
| `--team-id` | Apple Developer Team ID |
| `--archive` | Create distributable archive |

## `zero-native automate`

| Subcommand | Description |
| --- | --- |
| `list` | List automation-enabled apps |
| `snapshot` | Dump current app state |
| `screenshot` | Capture a screenshot |
| `reload` | Reload the WebView |
| `wait` | Wait for `ready=true` |
| `bridge <json>` | Send a bridge command |

## Environment Variables

| Variable | Description |
| --- | --- |
| `ZERO_NATIVE_FRONTEND_URL` | Dev server URL read by `frontend.sourceFromEnv` |
| `ZERO_NATIVE_FRONTEND_ASSETS` | App convention for signaling pre-built assets |
| `ZERO_NATIVE_LOG_DIR` | Override log output directory |
| `ZERO_NATIVE_LOG_FORMAT` | Log format: `text` or `jsonl` |
