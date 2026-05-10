# Frontend and Dev Server

Use this reference for frontend dev server setup, framework-specific commands, bundled assets, and production source variants. Keep the common `sourceFromEnv` API in `SKILL.md`.

## Source Switching Details

Use `source_fn` so development loads a localhost server and production loads assets:

```zig
fn source(context: *anyopaque) anyerror!zero_native.WebViewSource {
    const self: *App = @ptrCast(@alignCast(context));
    return zero_native.frontend.sourceFromEnv(self.env_map, .{
        .dist = "dist",
        .entry = "index.html",
    });
}
```

`sourceFromEnv` reads `ZERO_NATIVE_FRONTEND_URL`. If set, it returns a URL source. Otherwise it returns assets from the configured `dist`.

`frontend.Config`:

| Field | Default | Description |
| --- | --- | --- |
| `dist` | `"dist"` | Built frontend output |
| `entry` | `"index.html"` | Entry within dist |
| `origin` | `"zero://app"` | Asset origin |
| `spa_fallback` | `true` | Serve entry for unknown routes |
| `dev_url_env` | `"ZERO_NATIVE_FRONTEND_URL"` | Env var checked by `sourceFromEnv` |

For packaged builds that must always use local assets:

```zig
return zero_native.frontend.productionSource(.{ .dist = "dist", .entry = "index.html" });
```

## app.zon Frontend Config

```zig
.frontend = .{
    .dist = "dist",
    .entry = "index.html",
    .spa_fallback = true,
    .dev = .{
        .url = "http://127.0.0.1:5173/",
        .command = .{ "npm", "run", "dev", "--", "--host", "127.0.0.1" },
        .ready_path = "/",
        .timeout_ms = 30000,
    },
}
```

If the frontend lives under `frontend/`, prefer commands such as:

```zig
.command = .{ "npm", "--prefix", "frontend", "run", "dev" },
```

## Managed Dev Server Workflow

Use `zero-native dev` when zero-native should manage the frontend lifecycle. It starts the configured command, waits for the URL, launches the native shell with `ZERO_NATIVE_FRONTEND_URL`, sets `ZERO_NATIVE_HMR=1`, and stops the frontend when the shell exits.

```bash
zero-native dev --binary zig-out/bin/MyApp
zero-native dev --binary zig-out/bin/MyApp --url http://127.0.0.1:3000/ --command "npm run dev"
zero-native dev --binary zig-out/bin/MyApp --timeout-ms 60000
```

Flags:

| Flag | Description |
| --- | --- |
| `--manifest` | Path to `app.zon`; default `app.zon` |
| `--binary` | Compiled native binary; required |
| `--url` | Override manifest dev URL |
| `--command` | Override manifest dev command |
| `--timeout-ms` | Override readiness timeout |

Framework defaults:

| Framework | URL | Command |
| --- | --- | --- |
| Vite/React/Vue/Svelte | `http://127.0.0.1:5173/` | `npm run dev -- --host 127.0.0.1` |
| Next.js | `http://127.0.0.1:3000/` | `npm run dev -- --hostname 127.0.0.1` |
| Static preview | local preview URL | any local server command |

## Production Assets

Build and bundle frontend assets before packaging:

```bash
zig build bundle-assets
```

Production packages serve these through `zero://app/`, so paths like `/assets/app.js` work without `file://` URLs.

`ZERO_NATIVE_FRONTEND_ASSETS` is an app convention used by examples to signal pre-built assets; it is not read by the `frontend` module.
