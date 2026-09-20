# Releasing Goddard

Goddard ships signed in-app updates on macOS, Linux, and Windows. Releases live in
a **Cloudflare R2** bucket served at **`https://releases.goddardai.org`**. macOS uses
[Sparkle](https://sparkle-project.org), including binary deltas when available;
the native Linux and Windows updaters read architecture-specific feeds and
verify artifacts with the same EdDSA key. One release workflow produces all
platform artifacts and feeds.

Once set up, cutting a release is pushing a `v*` tag (or running the Release
workflow manually) — see [Cutting a release](#cutting-a-release). `bun run
release` only ever builds local artifacts; publishing is CI's job.

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
  copies published assets into the R2 bucket.

---

## One-time setup

The release runs on [Bun](https://bun.sh) and needs
[`create-dmg`](https://github.com/create-dmg/create-dmg) and
[rclone](https://rclone.org) (`brew install bun create-dmg rclone`).

### 1. Sparkle signing keys

Updates are signed with an ed25519 key; the private half stays in the login
keychain and the public half ships in Info.plist as `SUPublicEDKey`.

**This Mac already has the key** — Goddard signs with the same default-account
Sparkle key as kero, and the matching public key is already in Info.plist.
Nothing to do.

On a fresh machine, restore the key from the password-manager backup with the
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

### 3. Cloudflare R2 bucket + domain  ← **still to do once**

1. Create the bucket **`goddard-releases`** (Cloudflare dashboard → R2 → Create
   bucket). The release script will not create it — a bucket-scoped API token
   can't.
2. Attach the custom domain **`releases.goddardai.org`** to the bucket (bucket →
   Settings → Custom Domains). This serves objects publicly at
   `https://releases.goddardai.org/<file>`.
3. Make sure the R2 API token behind the `r2` rclone remote covers this bucket
   (R2 → Manage API Tokens → Object Read & Write). The remote already exists
   for kero; if `rclone lsf r2:goddard-releases --s3-no-check-bucket` returns
   *AccessDenied* after the bucket exists, extend the token's bucket list.

The rclone remote itself (`~/.config/rclone/rclone.conf`, type S3, provider
Cloudflare, `no_check_bucket = true`) is shared with kero and needs no change.

---

## Cutting a release

1. **Bump `version` in `Cargo.toml`** — the single source of truth.
   Until v1.0, always bump the **minor** version for a release (patch versions
   are reserved for hotfixes), so after `v0.2.x` the next release is `v0.3.0`.
   `CFBundleShortVersionString` is the version, and `CFBundleVersion` is
   derived from it (`major*1e6 + minor*1e3 + patch`, so `0.2.0` → `2000`),
   which keeps Sparkle's build-number comparison monotonic without a manual
   counter. Prerelease versions (`-beta.1`) become GitHub prereleases through
   CI: their versioned assets upload normally, but `sync-release` skips the
   appcasts and `latest-*` pointers, so the update feeds keep serving the
   stable channel.
2. **Write the release notes** — changes accumulate as fragments in
   `.changelog/` (one `.md` file per change, one bullet each, named
   `highlight-`/`feat-`/`exp-`/`fix-<slug>.md` to pick the `###` section; highlights
   must also commit a screenshot or recording at `.changelog/media/<slug>`
   and embed it via `![](media/<slug>.<ext>)`). Fold them into `CHANGELOG.md`:
   ```sh
   bun run changelog
   ```
   This creates the `## [<version>]` section for the Cargo version and deletes
   the consumed fragments. Commit it with the version bump.
3. **Release it through CI** — push a `v<version>` tag, or Actions → Release →
   Run workflow (see below). `bun run release` stays local-only: it builds,
   signs, notarizes, and writes the DMG + zip + appcast into `dist/`, which is
   what the workflow uploads as the GitHub release's assets and
   `sync-release.yml` mirrors to R2. To validate a release build by hand:
   ```sh
   bun run release --local
   ```

The script builds and signs the app via `scripts/bundle.sh release`, verifies
the bundled JS REPL and computer-use helper, builds the styled DMG, notarizes
and staples DMG + app, zips the app for Sparkle, attaches the changelog
section as release notes, and regenerates the signed `appcast.xml` when a
usable Sparkle key is present.

Test by keeping an older build around, launching it, and choosing
**Check for Updates…**.

### GitHub draft release + R2 sync

The Release workflow runs two ways:

- **Push a `v*` tag** — the tag must match the `version` in `Cargo.toml`, or the
  run fails before anything builds. A prerelease tag like `v0.2.0-beta.1`
  drafts a GitHub **prerelease**; publishing it uploads its assets but leaves
  the update feeds and `latest-*` pointers on the stable channel.
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
- The step pulls the live feeds down first and merges, so previously published
  releases keep their entries.

Both Linux jobs run on **Ubuntu 22.04**, and that choice is load-bearing: the
binaries link against the build machine's glibc, so the runner sets the oldest
distribution Goddard can start on (2.35 — Ubuntu 22.04, Debian 12, Fedora 36).
Moving those jobs to a newer runner silently drops support for everything
older.

The workflow opens (or updates) a **draft** GitHub release with those files and
the matching `CHANGELOG.md` section. Publishing the GitHub release syncs the
assets — including every signed update feed — to R2.

Every GitHub release's notes open with a **### Downloads** section — direct
links to the macOS DMG, the Windows installers and portable zips, and the Linux
tarballs plus the `install.sh` one-liner — above the changelog. The one-liner
pins the release's own tag so it always fetches a published script:
`curl -fsSL https://raw.githubusercontent.com/goddard-ai/goddard/<tag>/install.sh | sh`.
Keep it there when editing a draft's notes, and add it when cutting a release
by hand — but only on releases whose tag contains `install.sh` at the repo
root; older tags 404 and must not recommend it.

`appcast.xml`, the architecture-specific Linux/Windows appcasts,
`latest-linux.txt`, and `latest-windows.txt` are the bucket's mutable pointers
and upload with a short cache lifetime; everything else is versioned and
cached forever. Linux users install from that bucket via
[`install.sh`](install.sh), fetched from the repo's raw GitHub URL
(`https://raw.githubusercontent.com/goddard-ai/goddard/main/install.sh`) — see
[docs/linux.md](docs/linux.md).

Publishing that GitHub release (or running **Sync release** from Actions)
uploads the assets to the `goddard-releases` R2 bucket. Configure these repository
secrets first:

| Secret | Purpose |
| --- | --- |
| `GODDARD_ANALYTICS_ENDPOINT` | embedded in every desktop CI build |
| `GODDARD_ANALYTICS_WEBSITE_ID` | embedded in every desktop CI build |
| `GODDARD_SIGNING_IDENTITY` | Developer ID identity selector |
| `APPLE_CERTIFICATE` | base64-encoded Developer ID Application `.p12` |
| `APPLE_CERTIFICATE_PASSWORD` | password for that `.p12` |
| `APPLE_ID` | Apple ID used by `notarytool` |
| `APPLE_APP_SPECIFIC_PASSWORD` | app-specific password for that Apple ID |
| `APPLE_TEAM_ID` | Developer Team ID |
| `SPARKLE_PRIVATE_KEY` | EdDSA private key for `generate_appcast` |
| `WINDOWS_CERTIFICATE` | optional; base64-encoded Authenticode `.pfx` |
| `WINDOWS_CERTIFICATE_PASSWORD` | optional; password for that `.pfx` |
| `R2_ACCOUNT_ID` | Cloudflare account id for the R2 API |
| `R2_ACCESS_KEY_ID` | R2 Object Read & Write token |
| `R2_SECRET_ACCESS_KEY` | matching secret |
| `R2_BUCKET` | optional; defaults to `goddard-releases` |

### Options

| Flag / Env | Default | Purpose |
| --- | --- | --- |
| `--local` | — | accepted for CI clarity; every run is local |
| `--adhoc`, `--skip-notarize` | — | unsigned/notarization-free test builds |
| `--skip-build` | — | reuse existing release binaries |
| `--build-number <n>` / `GODDARD_BUILD_NUMBER` | derived | `CFBundleVersion` override |
| `GODDARD_DOWNLOAD_URL_PREFIX` | `https://releases.goddardai.org/` | base URL in the appcast |
| `SPARKLE_BIN` | the `~/Library/Caches/goddard-build` copy | Sparkle tools directory |
| `GODDARD_ANALYTICS_ENDPOINT`, `GODDARD_ANALYTICS_WEBSITE_ID` | — | embedded at build time; builds without them compile analytics out |
| `SPARKLE_PRIVATE_KEY` | login keychain | EdDSA key for `generate_appcast`; local builds skip the appcast when no usable key is found |

---

## Notes

- **Two artifacts per release:** the notarized `.dmg` (what people download)
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
- **Old archives stay in R2** so far-behind users can still be served; only
  the recent history is staged locally under `dist/updates/` (git-ignored).
- **Platform artifacts:** keep the bucket layout flat and platform-tagged by
  artifact name/extension — today's macOS names
  (`Goddard-<v>.dmg`, `Goddard-<v>.zip`, `appcast.xml`) must keep their URLs.
  Linux CI releases produce `Goddard-<v>-<target>.tar.gz` with
  `scripts/bundle-linux.sh`, Windows CI produces `Goddard-<v>-<target>.zip` with
  `scripts/bundle-windows.ts`, and both land in GitHub Releases, then R2 via
  the sync workflow. Windows also ships `Goddard-<v>-<arch>-Setup.exe`; each
  native client updates from `appcast-<platform>-<arch>.xml` while the Linux
  installer resolves `latest-linux.txt`. `src/updater.rs` is the per-platform
  seam, and everything
  mac-specific in the existing release pipeline lives behind the Darwin guard
  in `scripts/release.ts` plus `scripts/bundle.sh`.
