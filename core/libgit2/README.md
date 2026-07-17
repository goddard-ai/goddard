# libgit2

`@goddard-ai/libgit2` owns Goddard's direct integration with the libgit2 C library. It provides repository reads and narrowly scoped mutations through Bun FFI while leaving Git CLI workflows in the package that owns them.

## Package Boundary

- Import `git` from `@goddard-ai/libgit2` for supported repository operations.
- Import `createFakeGitApi` from `@goddard-ai/libgit2/testing` when a unit test needs an in-memory `GitApi`.
- Keep checkout, merge, rebase, fetch, patch serialization, and other porcelain workflows outside this package. The current subprocess inventory is tracked in [cli-coverage.md](./cli-coverage.md).
- Keep native source acquisition and compilation under [vendor/libgit2](./vendor/libgit2/README.md).

The public contract is documented in [the libgit2 package guide](../../docs/guides/libgit2-package.md) and in the exported types under `src/types.ts`.

## Runtime Model

The exported `git` object contains lazy namespaces. Accessing a namespace loads libgit2 once for the process, initializes it, and replaces the lazy property with the initialized namespace implementation. Importing the package alone does not load a native library.

The loader checks candidates in this order:

1. `GODDARD_GIT_LIBGIT2_PATH`.
2. `LIBGIT2_PATH`.
3. The target-specific artifact built under `vendor/libgit2/dist`.
4. The platform loader name, such as `libgit2.dylib`.
5. Conventional Homebrew and `/usr/local` library paths.

Packaged runtimes should pass an explicit target-specific path. Source-checkout fallbacks exist for development and tests; they are not a packaging contract.

Only `darwin-arm64` has a package-owned artifact definition today. Standalone daemon builds automatically fetch, build, verify, and stage the matching artifact; unsupported targets fail instead of producing a package that depends on host libgit2.

## Development

Build and verify the pinned native dependency before running the real-repository tests:

```sh
pnpm --dir core/libgit2/vendor/libgit2 run fetch
pnpm --dir core/libgit2/vendor/libgit2 run build -- --target darwin-arm64
pnpm --dir core/libgit2/vendor/libgit2 run verify -- --target darwin-arm64
pnpm --dir core/libgit2/vendor/libgit2 run prepare:runtime -- --target darwin-arm64
pnpm --dir core/libgit2 run test
```

Generated source, build, and artifact directories are ignored by Git. Use the vendor `clean` script to remove them.

## Extending The Binding

When adding an operation:

1. Add the smallest required C symbols and exact Bun FFI signatures in `src/libgit2/ffi.ts`.
2. Implement the capability in the matching lazy namespace in `src/libgit2/host.ts`.
3. Add its caller-facing contract to `src/types.ts` and the public package guide.
4. Verify ownership and disposal for every repository, object, reference, iterator, array, and buffer returned by libgit2.
5. Add a real-repository test that loads the built target artifact and exercises success, absence, and failure behavior where applicable.
6. Update [cli-coverage.md](./cli-coverage.md) when a consuming package no longer needs a Git subprocess.

Several reads use offsets into libgit2 v1 structs. Treat those offsets as target ABI constraints: document the corresponding struct field, keep target support explicit, and verify every new architecture against the pinned libgit2 version before adding it to the artifact manifest.

## Errors And Lifecycle

- `GitNotRepositoryError` means libgit2 could not open the supplied path as a repository.
- `GitHostError` covers native loading, invalid object IDs, and other libgit2 failures.
- Missing refs and config values return `null` only where the exported method contract says they do.
- `validateLibgit2Runtime()` eagerly checks that one candidate can be loaded and initialized.
- `resetGitForTests()` clears the process singleton and lazy namespaces. It is test lifecycle support, not a production reload mechanism.
