# Web Engines and Packaging

Use this reference for choosing system WebView vs Chromium/CEF, CEF setup, build overrides, packaging, signing, icons, updates, and distribution. For installation, scaffolding, and broad CLI command lookup, read [setup-scaffolding-cli.md](./setup-scaffolding-cli.md).

## Engine Choice

zero-native has one app/runtime API and selectable web engines.

| Platform | `system` | `chromium` |
| --- | --- | --- |
| macOS | WKWebView | CEF |
| Linux | WebKitGTK | Not wired yet; fails early |
| Windows | In progress | In progress |

Decision guide:

| Consideration | System | Chromium |
| --- | --- | --- |
| Bundle size | Minimal | Large |
| Rendering consistency | Varies by OS | Consistent for pinned CEF |
| Startup time | Fastest | Slower |
| Best fit | Small apps, OS-native footprint | Chromium behavior or strict rendering parity |

Set in `app.zon`:

```zig
.web_engine = "system",
```

or:

```zig
.web_engine = "chromium",
.cef = .{ .dir = "third_party/cef/macos", .auto_install = false },
```

## CEF Setup

Happy path:

```bash
zero-native cef install
zero-native doctor --manifest app.zon
```

Pin CEF versions in CI/setup notes:

```bash
zero-native cef install --version <version>
```

Use the same CEF layout for install, build, package, and verification. Mismatches usually appear as launch failures or missing framework/resource errors.

Allow automatic local setup only when appropriate:

```zig
.cef = .{ .dir = "third_party/cef/macos", .auto_install = true },
```

Advanced alternatives:

```bash
zero-native cef install --source official --allow-build-tools
zig build run -Dcef-dir=/path/to/cef
```

## Build Overrides

| Flag | Description |
| --- | --- |
| `-Dweb-engine=system` | Use system WebView for this build |
| `-Dweb-engine=chromium` | Use Chromium/CEF for supported platforms |
| `-Dcef-dir=path` | Override CEF directory |
| `-Dcef-auto-install=true` | Allow CEF install during Chromium builds |

CEF bundling:

```bash
zig build cef-bundle -Dcef-dir=/path/to/cef
```

Smoke tests:

```bash
zig build test-webview-cef-smoke -Dplatform=macos -Dweb-engine=chromium
zig build test-package-cef-layout -Dplatform=macos
```

## Packaging

Build and package:

```bash
zig build package
zero-native package --target macos --manifest app.zon --binary zig-out/bin/MyApp
```

Build options:

| Option | Values | Description |
| --- | --- | --- |
| `-Dplatform` | `auto`, `null`, `macos`, `linux` | Target platform |
| `-Dweb-engine` | `system`, `chromium` | Temporary engine override |
| `-Dcef-dir` | path | Temporary CEF override |
| `-Dtrace` | `off`, `events`, `runtime`, `all` | Trace level |
| `-Ddebug-overlay` | `true`, `false` | WebView debug overlay |
| `-Dautomation` | `true`, `false` | Automation server |
| `-Djs-bridge` | `true`, `false` | JavaScript bridge |

Package flags:

| Flag | Description |
| --- | --- |
| `--target` | `macos`, `linux`, `windows`, `ios`, `android` |
| `--manifest` | `app.zon` path |
| `--output` | Package output path |
| `--binary` | Built binary path |
| `--assets` | Frontend assets directory |
| `--optimize` | Optimization level |
| `--web-engine` | Temporary engine override |
| `--cef-dir` | Temporary CEF dir override |
| `--cef-auto-install` | Allow prepared CEF installation |
| `--signing` | `none`, `adhoc`, `identity` |
| `--identity` | Code signing identity |
| `--entitlements` | Entitlements file |
| `--team-id` | Apple Developer Team ID |
| `--archive` | Create distributable archive |

Platform status:

| Target | Status |
| --- | --- |
| `macos` | `.app`, signing, notarization, DMG |
| `linux` | Desktop entry, icons, binary package |
| `windows` | Early directory package |
| `ios` | Experimental |
| `android` | Experimental |

## macOS Signing and Notarization

Sign with:

```bash
zero-native package --target macos --signing identity --identity "Developer ID Application: Your Name"
```

Notarize manually after creating a signed archive:

```bash
xcrun notarytool submit zig-out/package/your-app.zip --apple-id "you@example.com" --team-id "TEAMID" --password "@keychain:AC_PASSWORD" --wait
xcrun stapler staple zig-out/package/your-app.app
```

For Chromium apps, sign and notarize after CEF has been bundled so helper executables and `Chromium Embedded Framework.framework` are covered.

Create a DMG:

```bash
zig build dmg
```

## Icons and Updates

Generate platform icon files from `assets/icon.png`:

```bash
zig build generate-icon
```

Declare update metadata in `app.zon`:

```zig
.updates = .{
    .feed_url = "https://example.com/releases/zero-native-feed.json",
    .public_key = "base64-ed25519-public-key",
    .check_on_start = true,
},
```

The runtime does not silently install updates. Apps should surface update checks in UI, verify signatures, and keep platform-specific installation explicit.
