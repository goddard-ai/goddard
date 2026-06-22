# Native libgit2

This folder owns the native libgit2 build pipeline for `@goddard-ai/git`. It fetches a pinned libgit2 source release, builds a target-specific shared library, verifies that Bun FFI can load it, and exposes the resulting artifact path for daemon packaging.

## Supported Targets

- `darwin-arm64` is the first supported target.
- Additional targets should add entries to `manifest.ts`, target-specific CMake settings when needed, and verification coverage before daemon packaging depends on them.

## Commands

- `pnpm --dir core/git/vendor/libgit2 run fetch` fetches the pinned libgit2 source into `source/libgit2`.
- `pnpm --dir core/git/vendor/libgit2 run build -- --target darwin-arm64` builds and installs the target artifact into `dist/darwin-arm64`.
- `pnpm --dir core/git/vendor/libgit2 run verify -- --target darwin-arm64` validates the artifact shape and loads it with Bun FFI.
- `pnpm --dir core/git/vendor/libgit2 run artifact -- --target darwin-arm64 --json` prints the absolute artifact path and manifest path for packaging scripts.
- `pnpm --dir core/git/vendor/libgit2 run clean` removes generated source, build, and dist output.

## Current Limitations

- The pipeline currently builds only `darwin-arm64`.
- Daemon compile and packaging do not yet consume this artifact automatically.
- Bun compiled-executable behavior is not wired yet; we still need to prove whether `dlopen` can load an embedded file directly or whether the daemon must extract the library to a real filesystem path first.
- Verification currently proves the library exists, has the expected architecture, and can call `git_libgit2_init`; it does not yet exercise every FFI symbol used by `@goddard-ai/git`.
- The libgit2 source is pinned by tag, but the pipeline does not yet enforce an expected commit checksum before building.
- The build disables SSH, HTTPS, and NTLM support for the initial local-repository use case. Add those features only with explicit dependency and packaging checks.
- macOS code signing, notarization, install-name validation, and app bundle integration are not handled here yet.
- Linux and Windows targets still need target entries, toolchain settings, dependency policy, and verification.

Generated directories are ignored by Git. Release automation should rebuild the target artifacts rather than relying on host-installed libgit2.
