# Native libgit2

This folder owns the native libgit2 build pipeline for `@goddard-ai/libgit2`. It fetches a commit-pinned libgit2 release, builds a target-specific shared library, verifies that Bun FFI can load it, and exposes a runtime-only artifact for daemon packaging.

## Supported Targets

- `darwin-arm64` is the first supported target.
- Additional targets should add entries to `manifest.ts`, target-specific CMake settings when needed, and verification coverage before daemon packaging depends on them.

## Commands

- `pnpm --dir core/libgit2/vendor/libgit2 run fetch` fetches the pinned libgit2 source into `source/libgit2`.
- `pnpm --dir core/libgit2/vendor/libgit2 run build -- --target darwin-arm64` builds and installs the target artifact into `dist/darwin-arm64`.
- `pnpm --dir core/libgit2/vendor/libgit2 run verify -- --target darwin-arm64` validates the artifact shape and loads it with Bun FFI.
- `pnpm --dir core/libgit2/vendor/libgit2 run prepare:runtime -- --target darwin-arm64` runs the complete fetch, build, and verification pipeline used by standalone daemon packaging.
- `pnpm --dir core/libgit2/vendor/libgit2 run artifact -- --target darwin-arm64 --json` prints the absolute artifact path and manifest path for packaging scripts.
- `pnpm --dir core/libgit2/vendor/libgit2 run clean` removes generated source, build, and dist output.

## Current Limitations

- The pipeline currently builds only `darwin-arm64`.
- Goddard's Bun FFI loader opens libgit2 from a filesystem path. The standalone runtime therefore carries it as a separate native file, and the desktop app installs both under the same versioned runtime directory.
- Verification currently proves the library exists, has the expected architecture, and can call `git_libgit2_init`; it does not yet exercise every FFI symbol used by `@goddard-ai/libgit2`.
- The build disables SSH, HTTPS, and NTLM support for the initial local-repository use case. Add those features only with explicit dependency and packaging checks.
- The macOS artifact receives an ad hoc signature for local loading. Release-identity signing and notarization remain responsibilities of the outer app packaging flow.
- Linux and Windows targets still need target entries, toolchain settings, dependency policy, and verification.

Generated directories are ignored by Git. Standalone daemon and desktop app builds rebuild the target artifact instead of relying on host-installed libgit2.
