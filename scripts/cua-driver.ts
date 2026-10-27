// Pinned SDK artifacts, shared by every packager and the dev watcher.
// Only libraries/support files are bundled; Goddard never runs cua-driver serve.
import { $ } from "bun";
import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import { cp, mkdir, mkdtemp, rename, rm, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { wakuCacheDir } from "./cache-dir";
import { writeCuaSkill } from "./cua-api";
import { prepareCuaHost } from "./cua-host";

export const cuaVersion = "0.38.0";
const root = resolve(import.meta.dir, "..");
const artifacts = {
  "darwin-arm64": [
    "5f71191c082ca07c9dfc13b7c9a6eec60a82818c245a93534cc0eb7911471d72",
    "libcua_driver_sdk.dylib",
  ],
  "darwin-x86_64": [
    "68721a916f2d9b20233a14623e0f663caf6130ba9018465a57a178120d96bc4e",
    "libcua_driver_sdk.dylib",
  ],
  "linux-arm64": [
    "ed8a51125ade8807c7717c3dde13d62bd31e6275e6b5e2446cf018d0b01d93b3",
    "libcua_driver_sdk.so",
  ],
  "linux-x86_64": [
    "a305cbd531f94d6413c5db300c33df68e3adbe8b92b9604cf8e645c5f6cb1fa3",
    "libcua_driver_sdk.so",
  ],
  "windows-arm64": [
    "c442872360cbb819e288b9f30daa7e3d2693d5945495d1400bcd90af5bd16402",
    "cua_driver_sdk.dll",
  ],
  "windows-x86_64": [
    "997f20114ad3db227a854e1e5a886234f76405b155f4af70605d4b7ca4a436a1",
    "cua_driver_sdk.dll",
  ],
} as const;

export function cuaPlatform(
  platform = process.platform,
  arch = process.arch,
): keyof typeof artifacts {
  const os = platform === "win32" ? "windows" : platform;
  const cpu = arch === "x64" ? "x86_64" : arch;
  const key = `${os}-${cpu}`;
  if (!(key in artifacts))
    throw new Error(`Cua Driver does not ship an SDK for ${key}`);
  return key as keyof typeof artifacts;
}

export async function prepareCuaSdk(platform = cuaPlatform()): Promise<string> {
  const [checksum, library] = artifacts[platform];
  const cacheRoot = join(wakuCacheDir(), "cua-driver", cuaVersion);
  const destination = join(cacheRoot, platform);
  const support = platform.startsWith("windows")
    ? "cua-driver-uia.exe"
    : "cua_driver_abi.h";
  if (
    existsSync(join(destination, library)) &&
    existsSync(join(destination, support))
  )
    return destination;
  await mkdir(cacheRoot, { recursive: true });
  const staging = await mkdtemp(join(cacheRoot, ".download-"));
  try {
    const name = `cua-driver-rs-${cuaVersion}-${platform}`;
    const extension = platform.startsWith("windows") ? "zip" : "tar.gz";
    const archive = join(staging, `sdk.${extension}`);
    const response = await fetch(
      `https://github.com/trycua/cua/releases/download/cua-driver-rs-v${cuaVersion}/${name}.${extension}`,
    );
    if (!response.ok)
      throw new Error(`Cua SDK download failed: ${response.status}`);
    const data = Buffer.from(await response.arrayBuffer());
    if (createHash("sha256").update(data).digest("hex") !== checksum)
      throw new Error("Cua SDK checksum mismatch");
    await writeFile(archive, data);
    // bsdtar ships with macOS and Windows; GNU tar handles the Linux archives.
    await $`tar -xf ${archive} -C ${staging}`.quiet();
    const unpacked = join(staging, name);
    for (const file of [library, "cua_driver_abi.h", support]) {
      if (!existsSync(join(unpacked, file)))
        throw new Error(`Cua SDK archive is missing ${file}`);
    }
    try {
      await rename(unpacked, destination);
    } catch (error) {
      if (!existsSync(join(destination, library))) throw error;
    }
  } finally {
    await rm(staging, { recursive: true, force: true });
  }
  return destination;
}

export async function bundleComputerUse(
  binDirectory: string,
  resourcesDirectory: string,
  profile: "debug" | "release",
  // The cargo target triple when cross-building; artifacts, helper binaries,
  // and the cua-host library all follow it rather than the host.
  targetTriple?: string,
): Promise<void> {
  const targetArch = targetTriple?.split("-")[0];
  const platform = cuaPlatform(
    undefined,
    targetArch === "aarch64" ? "arm64" : targetArch,
  );
  const sdk = await prepareCuaSdk(platform);
  const hostSdk = await prepareCuaHost(targetTriple);
  const target = resolve(
    root,
    process.env.CARGO_TARGET_DIR || "target",
    ...(targetTriple ? [targetTriple, profile] : [profile]),
  );
  const suffix = process.platform === "win32" ? ".exe" : "";
  await mkdir(binDirectory, { recursive: true });
  await mkdir(resourcesDirectory, { recursive: true });
  for (const file of [`goddard_js_repl${suffix}`, `goddard_computer_use${suffix}`]) {
    const destination = join(binDirectory, file);
    if (resolve(target, file) !== resolve(destination))
      await cp(join(target, file), destination);
  }
  await cp(
    join(hostSdk, artifacts[platform][1]),
    join(binDirectory, artifacts[platform][1]),
  );
  if (process.platform === "win32")
    await cp(
      join(sdk, "cua-driver-uia.exe"),
      join(binDirectory, "cua-driver-uia.exe"),
    );
  if (process.platform === "linux") {
    await cp(
      join(sdk, "wayland-helper"),
      join(resourcesDirectory, "computer-use", "wayland-helper"),
      { recursive: true },
    );
  }
  for (const [source, relative] of [
    ["resources/computer-use/CUA-LICENSE", "computer-use/CUA-LICENSE"],
  ]) {
    const destination = join(resourcesDirectory, relative!);
    await mkdir(dirname(destination), { recursive: true });
    await cp(join(root, source!), destination);
  }
  await writeCuaSkill(
    join(binDirectory, `goddard_computer_use${suffix}`),
    join(resourcesDirectory, "skills/goddard-computer-use/SKILL.md"),
  );
}

if (import.meta.main) {
  const [command, ...args] = process.argv.slice(2);
  if (command === "prepare") console.log(await prepareCuaSdk());
  else if (
    command === "bundle" &&
    args.length === 3 &&
    ["debug", "release"].includes(args[2]!)
  ) {
    await bundleComputerUse(
      resolve(args[0]!),
      resolve(args[1]!),
      args[2] as "debug" | "release",
    );
  } else
    throw new Error(
      "Usage: bun scripts/cua-driver.ts prepare | bundle <bin-dir> <resources-dir> <debug|release>",
    );
}
