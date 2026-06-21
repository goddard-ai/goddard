# Native libgit2

This folder owns Goddard's native libgit2 build pipeline. It fetches a pinned libgit2 source release, builds a target-specific shared library, verifies that Bun FFI can load it, and exposes the resulting artifact path for daemon packaging.

## Supported Targets

- `darwin-arm64` is the first supported target.
- Additional targets should add entries to `manifest.json`, target-specific CMake settings when needed, and verification coverage before daemon packaging depends on them.

## Commands

- `pnpm --dir native/libgit2 run fetch` fetches the pinned libgit2 source into `source/libgit2`.
- `pnpm --dir native/libgit2 run build -- --target darwin-arm64` builds and installs the target artifact into `dist/darwin-arm64`.
- `pnpm --dir native/libgit2 run verify -- --target darwin-arm64` validates the artifact shape and loads it with Bun FFI.
- `pnpm --dir native/libgit2 run artifact -- --target darwin-arm64 --json` prints the absolute artifact path and manifest path for packaging scripts.
- `pnpm --dir native/libgit2 run clean` removes generated source, build, and dist output.

Generated directories are ignored by Git. Release automation should rebuild the target artifacts rather than relying on host-installed libgit2.
