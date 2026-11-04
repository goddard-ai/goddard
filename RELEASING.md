# Releasing Goddard

Goddard ships signed in-app updates on macOS, Linux, and Windows. **GitHub
Releases** serves versioned artifacts and the stable appcast feeds. macOS uses
[Sparkle](https://sparkle-project.org), including binary deltas when available;
the native Linux and Windows updaters read architecture-specific feeds and
verify artifacts with the same EdDSA key. The final transition release is
copied once to Cloudflare R2 so older installed clients can take one last
update; new builds read GitHub's stable feeds directly.

`main` is the protected default branch for validated code; it may be ahead of
what is published. `dev` is a rough checkpoint branch and may be broken or
unfinished. Published release tags identify exactly what shipped.

Fast CI on `dev` prepares one draft release PR. Mark it ready to freeze the
candidate and run the full test matrix. Merging that PR builds a draft GitHub
release for human inspection. Publication remains a human action.
`bun run release` only builds local artifacts.

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
  merging the release PR, a `v*` tag, or a manual **Run workflow**, and opens
  a draft GitHub release;
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

1. **Wait for Fast CI on `dev`.** A successful push run creates or updates
   the draft `release/next` PR into `main`. Failed checkpoints are not promoted.
   Fast CI runs a Linux workspace check, protocol generation checks, TypeScript
   checks and tests, and release-automation tests.
2. **Review the entire candidate.** Passing CI does not mean all included
   features are finished. The PR preserves the source commits, merges current
   `main`, proposes the next minor version before 1.0 or patch version after
   1.0 (or uses a deliberately higher
   version from `dev`), updates `Cargo.lock`, and folds desktop and mobile
   changelog fragments. Inspect the version, notes, and complete diff. Until
   1.0, patch releases are reserved for deliberate hotfix version bumps.
3. **Mark ready for review.** Automatic updates stop. The macOS, Linux, and
   Windows workspace test matrix runs. New `dev` checkpoints do not change
   this candidate. Deliberate fixes pushed to `release/next` rerun checks;
   check both the code and release notes again before merging. Making the PR
   a draft again permits automatic updates only while its head is still an
   automation-generated commit. Manual commits are never overwritten.
4. **Merge after checks and human review.** Use **Create a merge commit**;
   do not squash or rebase the release PR. This preserves checkpoint ancestry
   and QA notes tied to original commit SHAs. `Fast checks` and `Release checks`
   must pass against current `main`. Direct pushes to `main` are blocked;
   Goddard's older direct QA promotion path cannot bypass this PR gate.
5. **Inspect the draft GitHub release.** The Release workflow builds and signs
   the exact merged revision for every supported platform. Check the attached
   packages, release notes, and download filenames, and test the packaged
   application. Draft download links become public after publication.
6. **Publish manually.** Publishing identifies the shipped version and lets
   automation prepare the next candidate from the latest passing checkpoint.
   No second candidate is prepared while a GitHub release remains a draft.
   Run the final R2 compatibility bridge only for the transition release.

If candidate preparation conflicts with `main`, the workflow fails without
changing the existing candidate. Reconcile `main` into `dev` without rewriting
QA-approved history, then push and let Fast CI run again. If a candidate has
manual fixes, finish it manually or close it and preserve its work before
removing `release/next`; automation refuses to reuse an unmerged closed branch.
A merged candidate branch can be reused automatically.

For a retry without a new checkpoint, use **Actions → Prepare release PR →
Run workflow**, selecting `main`. It selects the latest successful Fast CI push
run whose commit is still on `dev`; an older CI completion cannot rewind an
existing candidate. If `main` moves during preparation, retry the workflow.

### Repository setup

The default branch must be `main`. Protect it with required pull requests,
`Fast checks` and `Release checks`, strict status checks (current base required),
conversation resolution, and enforcement for administrators. Keep merge commits
enabled and linear-history enforcement disabled. No extra approving reviewer is
required: the maintainer reviews the bot-authored release PR and merges it.

Allow GitHub Actions to create pull requests in **Settings → Actions → General**.
The controller uses `GITHUB_TOKEN` and explicitly dispatches candidate CI, so
normal releases require no additional credentials or bot-created PR workflow
approval. If a pending automatic PR-event run asks for approval, the explicitly
dispatched Fast CI still runs; marking ready starts full CI through your action.

For candidates that change `.github/workflows/`, configure a GitHub App installed
on this repository with Contents, Pull requests, Actions, and Workflows write
permissions. Set repository variable `RELEASE_APP_ID` and secret
`RELEASE_APP_PRIVATE_KEY`. Without these optional credentials, GitHub may reject
an automatic branch update containing workflow changes; merge those workflow
changes through a maintainer-created PR instead. Never store a maintainer's
personal login token in the repository.

For initial installation, land the workflow and script changes on `main` through
a maintainer-created PR, run the pre-merge checks, and then dispatch **Prepare
release PR** on `main`. Thereafter it reacts to successful Fast CI push runs on
`dev` and manually published releases. Merely adding these files to `dev` does
not activate the default-branch controller.

The script builds and signs the app via `scripts/bundle.sh release`, verifies
the bundled JS REPL and computer-use helper, builds the styled DMG, notarizes
and staples DMG + app, zips the app for Sparkle, attaches the changelog
section as release notes, and regenerates the signed `appcast.xml` when a
usable Sparkle key is present.

Test by keeping an older build around, launching it, and choosing
**Check for Updates…**.

### GitHub release and final R2 compatibility bridge

The Release workflow runs on merging `release/next` into `main`, building the
exact merge commit. Two recovery paths remain available:

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
