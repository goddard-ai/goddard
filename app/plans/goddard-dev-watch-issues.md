# Goddard Dev Watch Issues

## Dev mode asks Electrobun to copy `dist` even though Vite serves the app

`app/electrobun.config.ts` always configures this copy rule:

```ts
copy: {
  dist: "views/main",
}
```

In development, `app/scripts/dev.ts` starts Vite and waits for the dev server. It does not run `vite build`, so `app/dist` normally does not exist.

Electrobun therefore logs:

```text
failed to copy app\dist because it doesn't exist.
```

The host usually survives this because `app/src/bun/index.ts` prefers the Vite dev server for the dev channel:

```text
http://127.0.0.1:5173
```

Impact: mostly non-fatal while Vite is running, but the fallback `views://main/index.html` cannot work if `dist` was not copied.

## Missing `dist` broadens Electrobun's watch scope to the app root

Because the configured copy source `dist` is missing, Electrobun watches the missing source's parent directory. That broadens the watch set to:

```text
app
```

Once the app root is watched recursively, Electrobun can observe its own build output under `app/build/...`.

Impact: this app config mismatch is the first link in the rebuild loop. It combines with Electrobun's Windows ignore bug to make generated files trigger rebuilds.

## Current failure chain

The observed loop is:

```text
missing app/dist
→ Electrobun watches app/
→ app/build changes are not ignored on Windows
→ Electrobun rebuilds from its own output/log changes
→ rebuild tries to remove app/build/dev-win-x64 while the app is active
→ Windows reports EACCES
```

The app-specific part is the dev-time `dist` copy expectation. The path-ignore and locked-process behavior appears to be in Electrobun.
