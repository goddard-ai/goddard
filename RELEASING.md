# Releasing Goddard

Goddard ships signed in-app updates on macOS, Linux, and Windows. **GitHub
Releases** serves versioned artifacts and the stable appcast feeds. macOS uses
[Sparkle](https://sparkle-project.org), including binary deltas when available;
the native Linux and Windows updaters read architecture-specific feeds and
verify artifacts with the same EdDSA key. The final transition release is
copied once to Cloudflare R2 so older installed clients can take one last
update; new builds read GitHub's stable feeds directly.

Once set up, cutting a release is the checklist in
[Cutting a release](#cutting-a-release): prep on `dev`, fast-forward `main` to
it, push a `v*` tag (or run the Release workflow manually), and stop at the
draft GitHub release for human review. `bun run release` only ever builds
local artifacts; publishing is CI's job.

- Updater code: [`src/updater.rs`](src/updater.rs) — loads the embedded
  Sparkle.framework on macOS and owns the signed native flows on Linux and
  Windows. Available updates appear in the sidebar footer; **Check for
  Updates…** lives in the app menu, and **Automatic updates** lives in
  Settings → General.
- Feed URL + public key: [`resources/Info.plist`](resources/Info.plist)
  (`SUFeedURL`, `SUPublicEDKey`).
- Framework embedding + pinned Sparkle version:
  [`scripts/bundle.sh`](scripts/bundle.sh) (bump `sparkle_version` and
  `sparkle_sha256` together; the distribution is cached under
  `~/Library/Caches/goddard-build/sparkle/`).
- Release automation: [`scripts/release.ts`](scripts/release.ts),
  [`scripts/appcast.ts`](scripts/appcast.ts),
  [`scripts/changelog.ts`](scripts/changelog.ts).
- GitHub Actions: [`.github/workflows/release.yml`](.github/workflows/release.yml)
  builds Linux (x86_64, arm64), Windows (x86_64, arm64), and macOS archives on
  a `v*` tag — or on a manual **Run workflow**, which takes the version from
  `Cargo.toml` — and opens a draft GitHub release;
  [`.github/workflows/sync-release.yml`](.github/workflows/sync-release.yml)
  is run manually once to publish the final R2 compatibility bridge.

---

## One-time setup

The release runs on [Bun](https://bun.sh) and needs
[`create-dmg`](https://github.com/create-dmg/create-dmg)
(`brew install bun create-dmg`). The bridge workflow installs and configures
[rclone](https://rclone.org) on its runner.

### 1. Sparkle signing keys

Updates are signed with an ed25519 key; the private half stays in the login
keychain and the public half ships in Info.plist as `SUPublicEDKey`.

Use the existing release signing key; generating a replacement would prevent
installed clients from trusting updates. On a fresh machine, restore the key
from the maintainers' backup with the
Sparkle tools (they land in `~/Library/Caches/goddard-build/sparkle/<version>/bin`
after any build, or download the release from
[sparkle-project/Sparkle](https://github.com/sparkle-project/Sparkle/releases)):

```sh
./bin/generate_keys -f sparkle_private_key.txt   # import the backed-up key
./bin/generate_keys -p                            # prints the public key — must
                                                  # match SUPublicEDKey
```

> ⚠️ Lose the private key and existing installs can never update again. Keep
> the backup current.

To split Goddard onto its own key later: `generate_keys --account goddard`, put the
new public key in Info.plist, and pass `--account goddard` through to
`generate_appcast` in `scripts/appcast.ts`. Users on old builds only trust the
old key, so do this on a release that still signs with the old key… in other
words, don't do it casually.

### 2. Developer ID signing + notarization

Copy `.env.example` to `.env` and replace the signing and analytics
placeholders. Bun loads these values before Cargo compiles the release, so the
analytics endpoint and website ID are embedded in the executable. The script
notarizes with the `NOTARY` keychain profile by default. On a fresh machine:

```sh
cp .env.example .env
xcrun notarytool store-credentials NOTARY \
  --apple-id you@example.com --team-id YOUR_APPLE_TEAM_ID
```

Override the environment with `--signing-identity`, or change the notary
profile with `--notary-profile` / `GODDARD_NOTARY_PROFILE`.

### 3. Final R2 compatibility bridge

The `goddard-releases` bucket and `releases.goddardai.org` custom domain are
needed only for the final transition release. The manual **Sync final R2
compatibility bridge** workflow copies that release's artifacts to the bucket
and rewrites its appcasts to R2 URLs, which older Linux clients still require.
New builds use GitHub for future updates; regular releases do not use this
bucket.

Before running the bridge, confirm the bucket serves objects at
`https://releases.goddardai.org/<file>` and that the repository has
`RELEASES_R2_ACCESS_KEY_ID` and `RELEASES_R2_SECRET_ACCESS_KEY` scoped to Object
Read & Write on `goddard-releases`. After the bridge succeeds, those release
credentials are no longer needed; keep the public bucket and domain available
while older clients may still fetch the transition release.

The bridge workflow configures its temporary rclone remote from the repository
secrets; no local rclone configuration is required.

---

## Cutting a release

All release prep lands on `dev`; `main` only ever fast-forwards to it, so it
never carries a commit `dev` lacks. The one exception is publishing the draft
GitHub release at the end — that stays a human's click.

1. **Check the branch relationship** before preparing the release:
   ```sh
   git fetch origin
   git merge-base --is-ancestor origin/main dev
   ```
   A zero exit status means `dev` contains the current published branch.
   If it fails, reconcile the branches before continuing. Keep `dev` in its
   own worktree, and preserve every commit through the newest QA approval:
   approvals under `refs/notes/qa` bind to commit SHAs and are lost if those
   commits are rewritten.
2. **Audit the changelog** — review the feature and fix commits since the last
   release tag:
   ```sh
   git log --oneline "$(git describe --tags --abbrev=0)"..dev
   git rev-parse dev   # the audited tip — saved for the delta check in step 7
   ```
   Give any changelog-worthy commit missing a `.changelog/` fragment one of its
   own, and check every pending fragment's filename: the category prefix
   (`highlight-`/`feat-`/`exp-`/`fix-`) picks the `###` section; Boss-related
   entries always fold into Experiments, including employee, deliverable,
   persona, and planning-session changes and fixes. Use `exp-` for new Boss
   fragments. An optional
   second segment — `<prefix>-<group>-<slug>.md`, with `group` one of
   `sessions`, `sidebar`, `composer`, `providers`, `git`, `transcript`,
   `panels`, `terminals`, `keyboard`, `navigation`, `appearance`,
   `permissions`, `settings`, `friends`, `ssh`, `platform` — files it under a
   `- **Group**` subsection. A fragment that only concerns the mobile app
   goes in `.changelog/mobile/` instead — same naming rules — and folds into
   `CHANGELOG.mobile.md`, keeping mobile-only notes out of the desktop
   changelog that feeds the in-app updater prompt. Rename mis-tagged
   fragments, then preview the fold:
   ```sh
   bun ./scripts/changelog.ts check
   ```
   An unrecognized group token lands the bullet in the flat tail, so check is
   how a mistyped group gets caught.
3. **Format the Rust code**:
   ```sh
   cargo fmt
   ```
   If it changes anything, commit the diff as `chore: format`. The TypeScript
   workspaces have no formatter — typecheck, below, is their gate.
4. **Run the tests** — everything CI runs, plus the mobile and web apps:
   ```sh
   bun install --frozen-lockfile
   cargo test --locked
   bun run protocol:check
   bun run --filter @waku/client check
   bun run --filter @waku/client test
   bun run --filter @waku/mobile typecheck
   bun run --filter @waku/mobile test
   bun run --filter @waku/web typecheck
   bun run --filter @waku/web test
   ```
   Land fixes for failing tests as their own `fix:` commits on `dev` — not
   folded into the release commit.
5. **Bump `version` in `Cargo.toml`** — the single source of truth.
   Until v1.0, always bump the **minor** version for a release (patch versions
   are reserved for hotfixes), so after `v0.2.x` the next release is `v0.3.0`.
   `CFBundleShortVersionString` is the version, and `CFBundleVersion` is
   derived from it (`major*1e6 + minor*1e3 + patch`, so `0.2.0` → `2000`),
   which keeps Sparkle's build-number comparison monotonic without a manual
   counter. Prerelease versions (`-beta.1`) become GitHub prereleases through
   CI. GitHub's `/releases/latest/download/` links continue to resolve to the
   latest stable release.
6. **Write the release notes** — changes accumulate as fragments in
   `.changelog/` (one `.md` file per change, one bullet each, named
   `highlight-`/`feat-`/`exp-`/`fix-<slug>.md` to pick the `###` section, or
   `<prefix>-<group>-<slug>.md` to also file under a `- **Group**`
   subsection; highlights
   must also commit a screenshot or recording at `.changelog/media/<slug>`
   and embed it via `![](media/<slug>.<ext>)`; mobile-only changes use
   `.changelog/mobile/` and fold into `CHANGELOG.mobile.md`). Fold them:
   ```sh
   bun run changelog
   ```
   This creates the `## [<version>]` section for the Cargo version in each
   changelog that has fragments and deletes the consumed ones. Before committing
   or tagging, review the new desktop and mobile sections as complete release
   notes. Describe what users can do or which problem is fixed, name the
   relevant screen, and include opt-in requirements or limits. Remove
   unexplained jargon, combine related entries, and check each claim against
   what actually ships. Commit the reviewed notes with the version bump
   (`chore: release v<version>`).
7. **Promote `dev` to `main`** — `dev` is a shared branch, so commits can land
   after the audit. Re-check the delta first, and give any new arrival the same
   fragment audit (a missed fragment just means a missing release-notes bullet —
   fix it in the draft). A final `cargo fmt --check` catches drift in code the
   earlier `cargo fmt` predates:
   ```sh
   git log --oneline <audited-tip>..dev
   cargo fmt --check
   ```
   Then fast-forward and push:
   ```sh
   git checkout main && git merge --ff-only dev && git push
   ```
   If CI fails after this, the fix lands on `dev` and `main` fast-forwards
   again — never commit to `main` directly.
8. **Release it through CI** — push a `v<version>` tag on the release commit
   (explicit SHA, not the `dev` ref, so the tag can't drift if `dev` moves
   again), or Actions → Release →
   Run workflow (see below). `bun run release` stays local-only: it builds,
   signs, notarizes, and writes the DMG + zip + appcast into `dist/`, which is
   what the workflow uploads as the GitHub release's assets. To validate a
   release build by hand:
   ```sh
   bun run release --local
   ```
   The workflow opens a **draft** GitHub release — stop there. The notes open
   with a `### Downloads` section the `draft-release` job writes itself; its
   links point to the draft's GitHub assets and become public when the release
   is published, so verify them against the attached files during review. After
   publishing the final transition release, run **Sync final R2 compatibility
   bridge** once with its tag so pre-migration clients can update.

The script builds and signs the app via `scripts/bundle.sh release`, verifies
the bundled JS REPL and computer-use helper, builds the styled DMG, notarizes
and staples DMG + app, zips the app for Sparkle, attaches the changelog
section as release notes, and regenerates the signed `appcast.xml` when a
usable Sparkle key is present.

Test by keeping an older build around, launching it, and choosing
**Check for Updates…**.

### GitHub release and final R2 compatibility bridge

The Release workflow runs two ways:

- **Push a `v*` tag** — the tag must match the `version` in `Cargo.toml`, or the
  run fails before anything builds. A prerelease tag like `v0.2.0-beta.1`
  drafts a GitHub **prerelease**; publishing it does not change the stable
  `/releases/latest/download/` appcast URLs.
- **Actions → Release → Run workflow** — no tag needed. The run releases
  whatever `Cargo.toml` says and drafts it as `v<version>`; that tag is created
  at the built commit when you publish the draft.

macOS CI runs `bun run release --local`, which signs, notarizes, and writes:

- `Goddard-<version>.dmg`
- `Goddard-<version>.zip`
- `appcast.xml` (Sparkle-signed)

Linux CI adds:

- `Goddard-<version>-x86_64-unknown-linux-gnu.tar.gz`
- `Goddard-<version>-aarch64-unknown-linux-gnu.tar.gz`
- `appcast-linux-x86_64.xml`, `appcast-linux-aarch64.xml`
- `latest-linux.txt` — the version `install.sh` resolves "latest" to

Windows CI adds:

- `Goddard-<version>-x86_64-Setup.exe`
- `Goddard-<version>-aarch64-Setup.exe`
- `Goddard-<version>-x86_64-pc-windows-msvc.zip` (portable)
- `Goddard-<version>-aarch64-pc-windows-msvc.zip` (portable)
- `appcast-windows-x86_64.xml`, `appcast-windows-aarch64.xml`
- `latest-windows.txt` — the version the download page resolves "latest" to

[`scripts/bundle-windows.ts`](scripts/bundle-windows.ts) builds both, driving
[`resources/windows/waku.iss`](resources/windows/waku.iss) through Inno Setup's
`ISCC`. The installer is **per-user** (`PrivilegesRequired=lowest`,
`%LOCALAPPDATA%\Programs\Goddard`) — no elevation, which is exactly what lets the
updater re-run it silently. The script signs the two executables and the
installer with Authenticode when `WINDOWS_CERTIFICATE` and
`WINDOWS_CERTIFICATE_PASSWORD` are set, and packages them unsigned otherwise,
so a fork without a certificate can still cut a release at the cost of a
SmartScreen warning.

**Never change `AppId` in `waku.iss`.** It is how Windows recognizes an
existing install; a new one turns every update into a second copy in
Add/Remove Programs.

#### The native Windows and Linux update feeds

Windows and Linux have no Sparkle, so [`src/updater.rs`](src/updater.rs) runs
the same contract itself: fetch the appcast, compare versions, download, and
verify the EdDSA signature. Windows hands the installer to Inno Setup with
`/SILENT`. Linux safely unpacks the tarball beside the managed user-local
prefix, then `goddard-updater` swaps it after the app's normal quit saves and
rolls back if the replacement cannot open its main window.

- **One feed per architecture.** A Sparkle appcast cannot say which binary an
  item is for, and the client picks its feed at compile time.
- **Same key as macOS.** `build.rs` reads `SUPublicEDKey` out of
  `resources/Info.plist` and compiles it in, so the three platforms cannot
  drift onto different keys.
- [`scripts/appcast-windows.ts`](scripts/appcast-windows.ts) and
  [`scripts/appcast-linux.ts`](scripts/appcast-linux.ts) sign the feeds in the
  draft-release job — the only one holding all native artifacts. They sign
  with Node's Ed25519 over the same `SPARKLE_PRIVATE_KEY`, and refuse to run
  when the key does not derive `SUPublicEDKey` (signing with the wrong key
  ships a feed the app rejects).
- The step downloads the latest published GitHub feeds and merges them, so
  previously published releases keep their entries. Historic R2 enclosure URLs
  are moved to the corresponding versioned GitHub assets.

Both Linux jobs run on **Ubuntu 22.04**, and that choice is load-bearing: the
binaries link against the build machine's glibc, so the runner sets the oldest
distribution Goddard can start on (2.35 — Ubuntu 22.04, Debian 12, Fedora 36).
Moving those jobs to a newer runner silently drops support for everything
older.

The workflow opens (or updates) a **draft** GitHub release with those files and
the matching `CHANGELOG.md` section, plus the `CHANGELOG.mobile.md` section
under a `### Mobile` heading when the release has one. GitHub serves the
versioned files and the stable feeds from
`https://github.com/goddard-ai/goddard/releases/latest/download/`.

The final transition release also needs to reach clients built before this
change. Those clients still request appcasts from `releases.goddardai.org`, and
older Linux updaters reject GitHub enclosure URLs. After publishing that one
release, run **Actions → Sync final R2 compatibility bridge** with its tag. The
workflow copies the release assets to R2 and rewrites its appcasts to R2 URLs.
The new build then checks GitHub for all later updates. Do not run the bridge
for subsequent releases.

| Installed build | Update feed | Update files |
| --- | --- | --- |
| Before the transition release | R2 bridge | R2-hosted transition assets |
| Transition release and later | GitHub `/releases/latest/download/` | Tag-specific GitHub release assets |

Every GitHub release's notes open with a **### Downloads** section — direct
links to the macOS DMG, the Windows installers and portable zips, and the Linux
tarballs plus the `install.sh` one-liner — above the changelog. The
`draft-release` job writes it; keep it when editing a draft's notes. The links
point at versioned GitHub assets and become public when the release is
published — verify their filenames against the attached assets during review.
The one-liner
pins the release's own tag so it always fetches a published script:
`curl -fsSL https://raw.githubusercontent.com/goddard-ai/goddard/<tag>/install.sh | sh`.
When cutting a release by hand, add the section yourself — but only on
releases whose tag contains `install.sh` at the repo root; older tags 404 and
must not recommend it.

`appcast.xml`, the architecture-specific Linux/Windows appcasts,
`latest-linux.txt`, and `latest-windows.txt` are GitHub release assets; the
`latest` download path selects them from the latest stable release. The final
R2 bridge copies these small pointers with a short cache lifetime; everything
else is versioned. Linux users install from GitHub via
[`install.sh`](install.sh), fetched from the repo's raw GitHub URL
(`https://raw.githubusercontent.com/goddard-ai/goddard/main/install.sh`) — see
[docs/linux.md](docs/linux.md).

Only the final transition release is copied to R2. Configure these repository
secrets before running that bridge:

| Secret | Purpose |
| --- | --- |
| `WAKU_POSTHOG_API_KEY` | PostHog project token embedded in every desktop CI build |
| `WAKU_POSTHOG_HOST` | Optional PostHog regional ingestion host override; defaults to EU Cloud |
| `GODDARD_SIGNING_IDENTITY` | Developer ID identity selector |
| `APPLE_CERTIFICATE` | base64-encoded Developer ID Application `.p12` |
| `APPLE_CERTIFICATE_PASSWORD` | password for that `.p12` |
| `APPLE_ID` | Apple ID used by `notarytool` |
| `APPLE_APP_SPECIFIC_PASSWORD` | app-specific password for that Apple ID |
| `APPLE_TEAM_ID` | Developer Team ID |
| `SPARKLE_PRIVATE_KEY` | EdDSA private key for `generate_appcast` |
| `WINDOWS_CERTIFICATE` | optional; base64-encoded Authenticode `.pfx` |
| `WINDOWS_CERTIFICATE_PASSWORD` | optional; password for that `.pfx` |
| `R2_ACCOUNT_ID` | Cloudflare account id for the R2 API; also used by sccache |
| `RELEASES_R2_ACCESS_KEY_ID` | bucket-scoped R2 Object Read & Write token |
| `RELEASES_R2_SECRET_ACCESS_KEY` | matching secret |
| `R2_BUCKET` | optional; defaults to `goddard-releases` |

### Options

| Flag / Env | Default | Purpose |
| --- | --- | --- |
| `--local` | — | accepted for CI clarity; every run is local |
| `--adhoc`, `--skip-notarize` | — | unsigned/notarization-free test builds |
| `--skip-build` | — | reuse existing release binaries |
| `--build-number <n>` / `GODDARD_BUILD_NUMBER` | derived | `CFBundleVersion` override |
| `GODDARD_DOWNLOAD_URL_PREFIX` | the current version's GitHub release URL | base URL in the appcast |
| `SPARKLE_BIN` | the `~/Library/Caches/goddard-build` copy | Sparkle tools directory |
| `GODDARD_POSTHOG_API_KEY` | — | PostHog project token embedded at build time; without it, analytics is disabled |
| `GODDARD_POSTHOG_HOST` | `https://eu.i.posthog.com` | regional ingestion host override |
| `SPARKLE_PRIVATE_KEY` | login keychain | EdDSA key for `generate_appcast`; local builds skip the appcast when no usable key is found |

---

## The dev channel

Settings → General can switch the updater between **Stable** (the bundle's
`SUFeedURL`, GitHub Releases) and **Dev** (`dev.goddardai.org`). The
pick persists in the app settings file and Sparkle consults it on every
check — switching back to Stable restores the production feed.

`bun run dev --serve` is the publisher: the same watcher, except it builds a
signed release `Goddard.app`, stamps `CFBundleVersion` with the derived
release number plus an epoch suffix (`2001.<seconds>` — ahead of the matching
release, behind the next one), zips it, regenerates `appcast.xml` signed with
the same Sparkle key releases use, and serves the directory through a named
Cloudflare tunnel to `dev.goddardai.org`. The tunnel (`goddard-dev`) and its
DNS route are created on first run when `cloudflared` is logged in; overrides:
`GODDARD_DEV_HOSTNAME`, `GODDARD_DEV_TUNNEL`, `GODDARD_DEV_SERVE_PORT`.

An update installed from the dev feed is a full signed release — the app it
lands on keeps working, and its own channel setting decides where it looks
next.

---

## Notes

- **macOS release artifacts:** the notarized `.dmg` (what people download)
  and a `.zip` (what Sparkle installs, plus `.delta` files against recent
  builds). Only the zip family appears in the appcast; point download buttons
  at the DMG.
- **Debug builds never update themselves.** `Updater::init` returns `None`
  under `debug_assertions`, so the dev watcher's app can't offer to replace
  itself with a production Goddard. Set `GODDARD_FORCE_UPDATER=1` to exercise the
  real Sparkle flow from a debug bundle anyway. A bare `cargo run` binary has
  no embedded framework and also degrades to no updater. For UI-only testing,
  start the watcher with `GODDARD_PREVIEW_UPDATE=1`; the sidebar immediately
  shows an available update and clicking it changes to the spinner without
  installing anything. The preview flag fakes only that sidebar result;
  **Check for Updates…** still uses the embedded Sparkle framework and its
  real standard window.
- **Automatic and explicit checks have separate presentation.** Scheduled
  checks stay silent until the sidebar update button appears. Choosing
  **Check for Updates…** promotes an existing silent result into Sparkle's
  standard updater window, or shows its checking progress while an automatic
  check finishes. With no automatic session active, it starts Sparkle's
  standard user-initiated check directly.
- **First-run consent:** Sparkle shows its one-time "check automatically?"
  prompt on the second launch. The Settings → General toggle reads and writes
  the same persisted value.
- **Goddard isn't sandboxed**, so Sparkle's XPC services are unnecessary;
  `bundle.sh` strips them (plus headers/modules) from the embedded framework
  and re-signs the rest with the app's identity — hardened-runtime library
  validation requires the identities to match.
- **Old archives stay in GitHub Releases.** The final R2 bridge copies the
  transition release and its appcasts for clients that still use the old host;
  regular releases go only to GitHub. Recent macOS history is staged locally
  under `dist/updates/` (git-ignored).
- **Platform artifacts:** keep GitHub release assets flat and platform-tagged
  by artifact name/extension — today's macOS names
  (`Goddard-<v>.dmg`, `Goddard-<v>.zip`, `appcast.xml`) must keep their URLs.
  Linux CI releases produce `Goddard-<v>-<target>.tar.gz` with
  `scripts/bundle-linux.sh`, Windows CI produces `Goddard-<v>-<target>.zip` with
  `scripts/bundle-windows.ts`, and both land in GitHub Releases. Windows also
  ships `Goddard-<v>-<arch>-Setup.exe`; each
  native client updates from `appcast-<platform>-<arch>.xml` while the Linux
  installer resolves `latest-linux.txt`. `src/updater.rs` is the per-platform
  seam, and everything
  mac-specific in the existing release pipeline lives behind the Darwin guard
  in `scripts/release.ts` plus `scripts/bundle.sh`.
