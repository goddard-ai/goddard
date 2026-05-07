# Electrobun Dev Watch Issues

## Windows icon embedding resolves `rcedit` to an upstream CI path

Electrobun attempts to stamp `app/assets/icon.png` into `launcher.exe` and `bun.exe`, but the `rcedit` executable path resolves to:

```text
D:\a\electrobun\electrobun\package\node_modules\rcedit\bin\rcedit-x64.exe
```

That drive does not exist on the local machine, so icon embedding fails with `ENOENT`.

The installed dependency does exist elsewhere in Bun's package store, so this looks like an Electrobun packaging or module-resolution issue on Windows rather than a missing app asset.

Impact: non-fatal. The generated executables launch, but their embedded icons are not updated.

## Built-in watch ignores do not match Windows paths

Electrobun says `build/`, `artifacts/`, and `node_modules/` are always ignored during `electrobun dev --watch`, but its ignore logic compares paths with `/` separators. On Windows, changed paths use `\`, so generated files under `app/build/...` are not ignored.

This allows changes like these to trigger rebuilds:

```text
app\build\dev-win-x64\Goddard-dev\Resources
app\build\dev-win-x64\Goddard-dev\bin\app.log
```

Impact: serious in watch mode. Electrobun can rebuild in response to its own output files and runtime logs.

## Watch directory deduplication appears separator-sensitive on Windows

The watch log includes both:

```text
app
app\src\bun
```

If path normalization were platform-robust, `app\src\bun` should be recognized as a child of `app` and deduped. This points to the same Windows path-separator class of issue as the built-in ignore failure.

Impact: minor by itself, but it supports the diagnosis that Electrobun's watch path logic is not fully Windows-normalized.

## Rebuild deletion does not fully account for locked child processes

During a rebuild, Electrobun kills the launched `launcher.exe` and then deletes the build folder. On Windows the launcher spawns a child `bun.exe`, and CEF/runtime files can remain locked. The delete step then fails:

```text
EACCES: permission denied, rm 'app\build\dev-win-x64'
```

Impact: fatal for that rebuild attempt. Watch mode continues, but subsequent generated-file changes can repeatedly hit the same failure.
