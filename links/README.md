# Develop with a local Octane checkout

Normal installs use published `@octane-xplat/*` packages. To temporarily use a
local checkout, run from this repository root, replacing `/path/to/octane-xplat`
with your checkout path:

Use a checkout compatible with Octane Xplat `0.16.1`, the version this app pins.
The app imports routing and stores from `@octane-xplat/ui/routing` and
`@octane-xplat/ui/data`. Unreleased checkouts may change these contracts.

Prepare the checkout's submodules, declarations, and CLI tools before linking.
Without generated UI declarations, the app checks framework implementation
sources directly and can report platform typing errors. [Octane PR #43](https://github.com/octane-xplat/octane-xplat/pull/43)
also fixes a keyboard type cycle under web-first suffix resolution.

```sh
git -C /path/to/octane-xplat submodule update --init --recursive
pnpm -C /path/to/octane-xplat install
pnpm -C /path/to/octane-xplat exec turbo run typegen --filter=@octane-xplat/ui
pnpm -C /path/to/octane-xplat exec turbo run build --filter=@octane-xplat/cli --filter=@octane-xplat/lint
ln -s /path/to/octane-xplat links/octane-xplat
pnpm dev:octane
```

The checkout link is ignored by Git. `dev:octane` links every declared root
`@octane-xplat/*` dependency to the matching package in the checkout, skipping
lifecycle scripts and leaving the committed manifest and lockfile unchanged.
A missing package or mismatched package name fails the install. Dependencies
must already be installed in the checkout; linked packages resolve their own
dependencies there.

Verify the links with:

```sh
realpath node_modules/@octane-xplat/{effects,jsx,platform,share,ui,cli,lint}
```

Every path should be under your checkout. To restore published packages, run:

```sh
pnpm install --frozen-lockfile
```

The link can stay in place. Remove it with `unlink links/octane-xplat` when no
longer needed. Ordinary installs and CI do not select the local hook.

Pnpm's pre-run dependency check is disabled, and the native dev/build scripts
skip NativeScript's install step, so they preserve the temporary links. Run
`pnpm install` explicitly after changing dependencies, then `pnpm dev:octane`
again to restore local links.

The app keeps its committed framework patches in both modes. This app uses
NativeScript Vite 8.0.17; newer Octane checkouts may no longer carry a patch for
that version. Use a checkout compatible with the
app's runtime versions, or upgrade the runtime and patches together.
