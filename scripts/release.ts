#!/usr/bin/env bun

import { $ } from "bun";
import {
  access,
  mkdir,
  mkdtemp,
  readdir,
  readlink,
  rm,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, extname, join, resolve } from "node:path";
import { parseArgs } from "node:util";
import { defaultDownloadUrlPrefix, generateAppcast } from "./appcast";
import { extractReleaseNotes } from "./changelog";

const appName = "Goddard";
const executableName = "Goddard";
const jsReplExecutableName = "goddard_js_repl";
const daemonExecutableName = "goddard-daemon";
const computerUseHelperName = "Goddard Computer Use";
const packageName = "waku";
const defaultNotaryProfile = "NOTARY";
const projectRoot = resolve(import.meta.dir, "..");

const help = `Build, notarize, and package a production release of Goddard.

Usage:
  bun run release [--local] [options]

Every run is local: the script builds a signed, notarized DMG, packages the
Sparkle update archive, and — when a Sparkle key is available — regenerates
the signed appcast into dist/. Publishing to Cloudflare R2 is the release
workflow's job: CI builds with this script, drafts a GitHub release from
dist/, and sync-release.yml mirrors it to the bucket. One-time setup lives
in RELEASING.md.

Options:
  --local                       Accepted for CI clarity; all builds are local
  --output <path>               DMG output path (default: dist/Goddard-<version>.dmg)
  --signing-identity <name>     Developer ID Application identity selector
                                (or GODDARD_SIGNING_IDENTITY; required unless --adhoc)
  --notary-profile <name>       notarytool keychain profile
                                (default: NOTARY; or GODDARD_NOTARY_PROFILE)
  --build-number <number>       CFBundleVersion override (or GODDARD_BUILD_NUMBER;
                                default derives a monotonic number from the
                                Cargo version)
  --volume-name <name>          Mounted DMG name (default: Goddard)
  --skip-build                  Reuse target/release/goddard, goddard_js_repl, and
                                goddard-daemon
  --skip-notarize               Unnotarized signed DMG
  --adhoc                       Ad-hoc sign, no notarization
  --help                        Show this help

Environment:
  GODDARD_SIGNING_IDENTITY         Developer ID Application identity selector
  GODDARD_ANALYTICS_ENDPOINT       analytics endpoint embedded at build time
  GODDARD_ANALYTICS_WEBSITE_ID     analytics website ID embedded at build time
                                (unset builds compile analytics out)
  GODDARD_DOWNLOAD_URL_PREFIX      base URL the appcast links to
                                (default: ${defaultDownloadUrlPrefix})
  SPARKLE_BIN                   Sparkle tools dir (default: the bundle.sh cache
                                under ~/Library/Caches/goddard-build/sparkle)
  SPARKLE_PRIVATE_KEY           Sparkle EdDSA private key (otherwise keychain);
                                the appcast step is skipped when no usable
                                key is found

Before the first release:
  xcrun notarytool store-credentials NOTARY   # notarization credentials
  See RELEASING.md for the R2 bucket, rclone remote, and Sparkle key setup.
`;

const { values } = parseArgs({
  args: Bun.argv.slice(2),
  options: {
    adhoc: { type: "boolean" },
    "build-number": { type: "string" },
    help: { type: "boolean", short: "h" },
    local: { type: "boolean" },
    "notary-profile": { type: "string" },
    output: { type: "string", short: "o" },
    "signing-identity": { type: "string" },
    "skip-build": { type: "boolean" },
    "skip-notarize": { type: "boolean" },
    "volume-name": { type: "string" },
  },
  strict: true,
});

if (values.help) {
  console.log(help);
  process.exit(0);
}

if (process.platform !== "darwin") {
  throw new Error("DMG packaging must run on macOS.");
}

function requireTool(name: string): void {
  if (!Bun.which(name)) {
    throw new Error(`Required tool not found in PATH: ${name}`);
  }
}

function logStep(message: string): void {
  console.log(`\n==> ${message}`);
}

type CargoMetadata = {
  packages: Array<{
    name: string;
    version: string;
  }>;
};

/** CFBundleVersion derived from the Cargo version. Sparkle decides which of
 *  two builds is newer by comparing this value, so it must grow with every
 *  release: three digits per semver field keep 0.2.0 → 2000 ahead of
 *  0.1.9 → 1009, and every release ahead of the pre-Sparkle DMGs that
 *  shipped CFBundleVersion 1. */
function derivedBuildNumber(version: string): string {
  const match = version.match(/^(\d{1,3})\.(\d{1,3})\.(\d{1,3})(?:-|$)/);
  const major = Number(match?.[1]);
  const minor = Number(match?.[2]);
  const patch = Number(match?.[3]);
  if (![major, minor, patch].every(Number.isInteger)) {
    throw new Error(
      `Cannot derive a build number from version "${version}"; ` +
        "pass --build-number.",
    );
  }
  return String(major * 1_000_000 + minor * 1_000 + patch);
}

const adhoc = values.adhoc ?? false;
const skipNotarize = values["skip-notarize"] ?? false;
const configuredSigningIdentity =
  values["signing-identity"] ?? process.env.GODDARD_SIGNING_IDENTITY;
const notaryProfile =
  values["notary-profile"] ??
  process.env.GODDARD_NOTARY_PROFILE ??
  defaultNotaryProfile;
const explicitBuildNumber =
  values["build-number"] ?? process.env.GODDARD_BUILD_NUMBER;
const analyticsEndpoint = process.env.GODDARD_ANALYTICS_ENDPOINT?.trim();
const analyticsWebsiteId = process.env.GODDARD_ANALYTICS_WEBSITE_ID?.trim();
const downloadUrlPrefix =
  process.env.GODDARD_DOWNLOAD_URL_PREFIX ?? defaultDownloadUrlPrefix;

if (adhoc && values["signing-identity"]) {
  throw new Error("Use either --adhoc or --signing-identity, not both.");
}
if (!adhoc && !configuredSigningIdentity) {
  throw new Error(
    "Set GODDARD_SIGNING_IDENTITY or pass --signing-identity (or use --adhoc).",
  );
}
if (explicitBuildNumber && !/^\d+(?:\.\d+){0,2}$/.test(explicitBuildNumber)) {
  throw new Error(
    "--build-number must contain one to three period-separated integers.",
  );
}
if (!values["skip-build"] && (!analyticsEndpoint || !analyticsWebsiteId)) {
  console.warn(
    "GODDARD_ANALYTICS_ENDPOINT/GODDARD_ANALYTICS_WEBSITE_ID unset — " +
      "building with analytics disabled.",
  );
}

for (const tool of [
  "cargo",
  "codesign",
  "create-dmg",
  "diskutil",
  "ditto",
  "plutil",
  "xattr",
]) {
  requireTool(tool);
}
if (!adhoc && !skipNotarize) {
  requireTool("xcrun");
  requireTool("spctl");
}
process.chdir(projectRoot);

const metadata = JSON.parse(
  await $`cargo metadata --no-deps --format-version 1`.quiet().text(),
) as CargoMetadata;
const cargoPackage = metadata.packages.find(
  (candidate) => candidate.name === packageName,
);
if (!cargoPackage) {
  throw new Error(`Cargo package "${packageName}" was not found.`);
}

const version = cargoPackage.version;
const shortVersion = version.split("-", 1)[0];
const buildNumber = explicitBuildNumber ?? derivedBuildNumber(version);
const dmgName = `${appName}-${version}.dmg`;
const zipName = `${appName}-${version}.zip`;

const outputPath = resolve(
  projectRoot,
  values.output ?? join("dist", dmgName),
);
const volumeName = values["volume-name"] ?? appName;
const releaseDirectory = resolve(
  projectRoot,
  process.env.CARGO_TARGET_DIR ?? "target",
  "release",
);
const releaseExecutable = join(releaseDirectory, packageName);
const releaseJsReplExecutable = join(
  releaseDirectory,
  jsReplExecutableName,
);
const releaseDaemonExecutable = join(releaseDirectory, daemonExecutableName);
const appBundle = join(releaseDirectory, `${appName}.app`);
const contentsDirectory = join(appBundle, "Contents");
const bundledJsReplExecutable = join(
  contentsDirectory,
  "Resources",
  jsReplExecutableName,
);
const bundledDaemonExecutable = join(
  contentsDirectory,
  "MacOS",
  daemonExecutableName,
);
const bundledComputerUseSkill = join(
  contentsDirectory,
  "Resources",
  "skills",
  "goddard-computer-use",
  "SKILL.md",
);
const bundledPiComputerUseExtension = join(
  contentsDirectory,
  "Resources",
  "computer-use",
  "pi-extension.ts",
);
const bundledComputerUseHelper = join(
  contentsDirectory,
  "Helpers",
  `${computerUseHelperName}.app`,
);
const bundledSparkleFramework = join(
  contentsDirectory,
  "Frameworks",
  "Sparkle.framework",
);

async function verifyJavaScriptRepl(executable: string): Promise<void> {
  const child = Bun.spawn([executable], {
    stdin: "pipe",
    stdout: "pipe",
    stderr: "pipe",
  });
  const requests = [
    {
      jsonrpc: "2.0",
      id: 1,
      method: "initialize",
      params: {
        protocolVersion: "2025-06-18",
        capabilities: {},
        clientInfo: { name: "goddard-release", version: "1" },
      },
    },
    { jsonrpc: "2.0", id: 2, method: "tools/list", params: {} },
    {
      jsonrpc: "2.0",
      id: 3,
      method: "tools/call",
      params: {
        name: "js",
        arguments: { code: "jsRepl.write(typeof cua);" },
      },
    },
  ];
  child.stdin.write(
    `${requests.map((request) => JSON.stringify(request)).join("\n")}\n`,
  );
  child.stdin.end();
  const stdout = await new Response(child.stdout).text();
  const stderr = await new Response(child.stderr).text();
  const exitCode = await child.exited;
  if (exitCode !== 0) {
    throw new Error(
      `Bundled JavaScript REPL exited with ${exitCode}: ${stderr.trim()}`,
    );
  }
  const responses = stdout
    .split("\n")
    .filter(Boolean)
    .map((line) => JSON.parse(line));
  const tools = responses
    .find((response) => response.id === 2)
    ?.result?.tools?.map((tool: { name?: string }) => tool.name);
  if (JSON.stringify(tools) !== JSON.stringify(["js", "js_reset"])) {
    throw new Error(
      `Bundled JavaScript REPL exposed unexpected tools: ${stdout}`,
    );
  }
  const lazySky = responses.find((response) => response.id === 3)
    ?.result?.content?.[0]?.text;
  if (lazySky !== "undefined") {
    throw new Error(`Bundled JavaScript REPL initialized sky eagerly: ${stdout}`);
  }
}

/** Detaches interstitial images left attached by failed create-dmg runs
 *  (their image paths are named rw.<pid>.<name>.dmg). */
async function detachStaleDmgImages(): Promise<void> {
  const info = await $`hdiutil info`.quiet().text();
  for (const block of info.split(/^=+$/m)) {
    if (!/image-path\s+:.*\/rw\.[^/\s]+\.dmg/.test(block)) {
      continue;
    }
    const device = block.match(/^\/dev\/disk\d+\s/m)?.[0].trim();
    if (device) {
      await $`hdiutil detach ${device} -force`.quiet().nothrow();
    }
  }
}

/** Removes create-dmg's rw.*.dmg interstitial files next to the output DMG. */
async function removeStaleDmgTempFiles(): Promise<void> {
  for (const entry of await readdir(dirname(outputPath))) {
    if (/^rw\..*\.dmg$/.test(entry)) {
      await rm(join(dirname(outputPath), entry), { force: true });
    }
  }
}

if (extname(outputPath).toLowerCase() !== ".dmg") {
  throw new Error(`Output path must end in .dmg: ${outputPath}`);
}
if (
  !volumeName.trim() ||
  volumeName.includes("/") ||
  volumeName.length > 27
) {
  throw new Error(
    "--volume-name must be non-empty, at most 27 characters, and cannot contain '/'.",
  );
}

let temporaryDirectory: string | undefined;
let mountedDmg = false;
let mountDirectory: string | undefined;
const identity = adhoc ? "-" : configuredSigningIdentity!;

try {
  if (values["skip-build"]) {
    for (const executable of [
      releaseExecutable,
      releaseJsReplExecutable,
      releaseDaemonExecutable,
    ]) {
      try {
        await access(executable);
      } catch {
        throw new Error(
          `Release executable not found at ${executable}. ` +
            "Run without --skip-build first.",
        );
      }
    }
  }

  logStep(
    values["skip-build"]
      ? "Assembling the app bundle"
      : "Building and assembling the app bundle",
  );
  await $`env GODDARD_CODESIGN_IDENTITY=${identity} GODDARD_ANALYTICS_ENDPOINT=${analyticsEndpoint ?? ""} GODDARD_ANALYTICS_WEBSITE_ID=${analyticsWebsiteId ?? ""} GODDARD_SKIP_CARGO_BUILD=${values["skip-build"] ? "1" : "0"} ${join(projectRoot, "scripts", "bundle.sh")} release`;
  for (const artifact of [
    join(contentsDirectory, "MacOS", executableName),
    bundledDaemonExecutable,
    bundledJsReplExecutable,
    bundledComputerUseSkill,
    bundledPiComputerUseExtension,
    bundledComputerUseHelper,
    join(bundledSparkleFramework, "Sparkle"),
  ]) {
    await access(artifact);
  }
  await $`plutil -replace CFBundleShortVersionString -string ${shortVersion} ${join(contentsDirectory, "Info.plist")}`;
  await $`plutil -replace CFBundleVersion -string ${buildNumber} ${join(contentsDirectory, "Info.plist")}`;
  await $`xattr -cr ${appBundle}`;

  await $`codesign --verify --strict --verbose=2 ${bundledJsReplExecutable}`;
  await $`codesign --verify --strict --verbose=2 ${bundledDaemonExecutable}`;
  await $`codesign --verify --deep --strict --verbose=2 ${bundledComputerUseHelper}`;
  await verifyJavaScriptRepl(bundledJsReplExecutable);
  logStep(
    adhoc
      ? "Ad-hoc signing the final app bundle"
      : `Signing the final app bundle as ${identity}`,
  );
  if (adhoc) {
    // No hardened runtime here: an ad-hoc identity carries no Team ID, so
    // library validation would refuse the embedded Sparkle framework and the
    // updater could never be exercised from an ad-hoc build.
    await $`codesign --force --sign - ${appBundle}`;
  } else {
    await $`codesign --force --options runtime --timestamp --sign ${identity} ${appBundle}`;
  }
  await $`codesign --verify --deep --strict --verbose=2 ${appBundle}`;

  temporaryDirectory = await mkdtemp(join(tmpdir(), "goddard-dmg-"));
  const stagingDirectory = join(temporaryDirectory, "root");
  mountDirectory = join(temporaryDirectory, "mount");
  await mkdir(stagingDirectory);
  // ditto, not fs.cp: fs.cp rewrites the Sparkle framework's relative
  // symlinks into absolute paths under target/, which breaks the framework
  // on any other machine and fails the deep verify below.
  await $`ditto ${appBundle} ${join(stagingDirectory, `${appName}.app`)}`;
  await mkdir(dirname(outputPath), { recursive: true });
  await rm(outputPath, { force: true });

  // Finder draws a DMG background at one point per pixel, so a single PNG
  // would clip on Retina displays. tiffutil packs the 1x and 2x sources into
  // one multi-representation TIFF and Finder picks per display. The window
  // is sized to the artwork's logical size, 700x425, and the icon positions
  // below are laid out against it.
  const dmgBackground = join(temporaryDirectory, "dmg-background.tiff");
  await $`tiffutil -cathidpicheck resources/dmg-background.png resources/dmg-background@2x.png -out ${dmgBackground}`;

  logStep(`Creating the styled DMG at ${outputPath}`);
  // APFS mounting races create-dmg: hdiutil attach can return before the
  // synthesized volume device exists, so create-dmg picks the APFS container
  // partition, finds no mount point for it, and fails with "interstitial
  // disk image was not found" while leaving the image attached. Detach the
  // leaked image, drop its rw.*.dmg temp file, and retry.
  for (let attempt = 1; ; attempt++) {
    const result =
      await $`create-dmg --volname ${volumeName} --window-pos 200 120 --window-size 700 425 --text-size 13 --icon-size 128 --icon ${`${appName}.app`} 190 189 --hide-extension ${`${appName}.app`} --app-drop-link 510 189 --background ${dmgBackground} --filesystem APFS --format ULFO --no-internet-enable --overwrite ${outputPath} ${stagingDirectory}`
        .quiet()
        .nothrow();
    if (result.exitCode === 0) {
      process.stdout.write(result.stdout);
      break;
    }
    process.stdout.write(result.stdout);
    process.stderr.write(result.stderr);
    if (attempt === 3) {
      throw new Error(`create-dmg failed after ${attempt} attempts.`);
    }
    console.warn(`create-dmg failed; cleaning up and retrying (${attempt}/3).`);
    await detachStaleDmgImages();
    await removeStaleDmgTempFiles();
  }

  logStep(adhoc ? "Ad-hoc signing the DMG" : "Signing the DMG");
  if (adhoc) {
    await $`codesign --force --sign - ${outputPath}`;
  } else {
    await $`codesign --force --timestamp --sign ${identity} ${outputPath}`;
  }
  await $`codesign --verify --verbose=2 ${outputPath}`;

  logStep("Verifying the DMG contents");
  await mkdir(mountDirectory);
  await $`diskutil image attach --readOnly --mountOptions nobrowse --mountPoint ${mountDirectory} ${outputPath}`;
  mountedDmg = true;
  const mountedApp = join(mountDirectory, `${appName}.app`);
  const mountedContents = join(mountedApp, "Contents");
  const mountedJsRepl = join(
    mountedContents,
    "Resources",
    jsReplExecutableName,
  );
  const mountedDaemon = join(
    mountedContents,
    "MacOS",
    daemonExecutableName,
  );
  const mountedComputerUseHelper = join(
    mountedContents,
    "Helpers",
    `${computerUseHelperName}.app`,
  );
  const mountedSparkleFramework = join(
    mountedContents,
    "Frameworks",
    "Sparkle.framework",
  );
  for (const artifact of [
    join(mountedContents, "MacOS", executableName),
    mountedDaemon,
    mountedJsRepl,
    join(
      mountedContents,
      "Resources",
      "skills",
      "goddard-computer-use",
      "SKILL.md",
    ),
    join(
      mountedContents,
      "Resources",
      "computer-use",
      "pi-extension.ts",
    ),
    mountedComputerUseHelper,
    join(mountedSparkleFramework, "Sparkle"),
  ]) {
    await access(artifact);
  }
  await access(join(mountDirectory, ".DS_Store"));
  await access(join(mountDirectory, ".background", "dmg-background.tiff"));
  const applicationsTarget = await readlink(
    join(mountDirectory, "Applications"),
  );
  if (applicationsTarget !== "/Applications") {
    throw new Error(
      `DMG Applications link points to "${applicationsTarget}", expected "/Applications".`,
    );
  }
  await $`codesign --verify --strict --verbose=2 ${mountedJsRepl}`;
  await $`codesign --verify --strict --verbose=2 ${mountedDaemon}`;
  await $`codesign --verify --deep --strict --verbose=2 ${mountedComputerUseHelper}`;
  await $`codesign --verify --strict --verbose=2 ${mountedSparkleFramework}`;
  await $`codesign --verify --deep --strict --verbose=2 ${mountedApp}`;
  await verifyJavaScriptRepl(mountedJsRepl);
  await $`diskutil eject ${mountDirectory}`;
  mountedDmg = false;

  if (!adhoc && !skipNotarize) {
    logStep("Submitting the DMG for Apple notarization");
    const resultText =
      await $`xcrun notarytool submit ${outputPath} --keychain-profile ${notaryProfile!} --wait --output-format json`
        .quiet()
        .text();
    const result = JSON.parse(resultText) as {
      id?: string;
      message?: string;
      status?: string;
    };
    if (result.status !== "Accepted") {
      throw new Error(
        `Notarization ${result.status ?? "failed"}${result.id ? ` (${result.id})` : ""}: ` +
          (result.message ?? "inspect the submission with notarytool log"),
      );
    }
    console.log(`Notarization accepted: ${result.id ?? "unknown submission"}`);

    logStep("Stapling and assessing the notarized DMG");
    await $`xcrun stapler staple -v ${outputPath}`;
    await $`xcrun stapler validate -v ${outputPath}`;
    await $`spctl --assess --type open --context context:primary-signature --verbose=2 ${outputPath}`;
    // Notarizing the DMG also notarized the app's code, so the same
    // submission staples the app for the Sparkle archive.
    logStep("Stapling the app for the update archive");
    await $`xcrun stapler staple -v ${appBundle}`;
  } else if (adhoc) {
    console.warn(
      "\nCreated an ad-hoc signed DMG. It is suitable for local testing only.",
    );
  } else {
    console.warn(
      "\nCreated a Developer ID-signed DMG without notarization. " +
        "Gatekeeper will reject it on other Macs until it is notarized.",
    );
  }

  const zipPath = resolve(projectRoot, "dist", zipName);
  await mkdir(dirname(zipPath), { recursive: true });
  logStep(`Packaging ${zipName}`);
  await $`ditto -c -k --keepParent ${appBundle} ${zipPath}`;

  // A clean staging directory holds this release's archive for
  // generate_appcast.
  const updatesDirectory = join(projectRoot, "dist", "updates");
  await rm(updatesDirectory, { force: true, recursive: true });
  await mkdir(updatesDirectory, { recursive: true });

  await $`ditto ${zipPath} ${join(updatesDirectory, zipName)}`;

  // Release notes: this version's CHANGELOG.md section ships next to the
  // archive as Goddard-<version>.md; generate_appcast links it as the update's
  // release notes, which Sparkle renders in the prompt.
  const changelogFile = Bun.file(join(projectRoot, "CHANGELOG.md"));
  const notes = (await changelogFile.exists())
    ? extractReleaseNotes(await changelogFile.text(), version)
    : null;
  const notesName = `${appName}-${version}.md`;
  const notesContents = `${notes ?? "See CHANGELOG.md for details."}\n`;
  await Bun.write(join(updatesDirectory, notesName), notesContents);
  // The tag workflow publishes files from dist/ as GitHub release assets;
  // sync-release then mirrors those assets to R2. Keep the notes beside the
  // appcast there as well so Sparkle's release-notes URL cannot 404.
  await Bun.write(join(projectRoot, "dist", notesName), notesContents);
  console.log(
    notes
      ? `Attached release notes for ${version}.`
      : `No "${version}" section in CHANGELOG.md — attached fallback notes.`,
  );

  // A signed feed is a nicety, not a requirement: keep producing it when a
  // usable Sparkle key is around (CI exports SPARKLE_PRIVATE_KEY and uploads
  // the result), and warn instead of failing when there isn't one.
  try {
    logStep("Generating the signed appcast");
    await generateAppcast(updatesDirectory, downloadUrlPrefix);
    await $`ditto ${join(updatesDirectory, "appcast.xml")} ${join(projectRoot, "dist", "appcast.xml")}`;
  } catch (error) {
    await rm(join(projectRoot, "dist", "appcast.xml"), { force: true });
    console.warn(
      `Skipping the update feed: ${error instanceof Error ? error.message : error}`,
    );
  }

  console.log(`\nDMG ready: ${outputPath}`);
  console.log(`ZIP ready: ${zipPath}`);
} finally {
  if (mountedDmg && mountDirectory) {
    const result = await $`diskutil eject ${mountDirectory}`.quiet().nothrow();
    if (result.exitCode === 0) {
      mountedDmg = false;
    } else {
      console.warn(`Unable to detach temporary mount at ${mountDirectory}.`);
    }
  }
  if (temporaryDirectory && !mountedDmg) {
    await rm(temporaryDirectory, { force: true, recursive: true });
  } else if (temporaryDirectory) {
    console.warn(`Temporary files retained at ${temporaryDirectory}.`);
  }
}
