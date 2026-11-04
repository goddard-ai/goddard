import { existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

export function packageVersion(text: string): string {
  const version = text.match(/^version = "([^"]+)"/m)?.[1];
  if (!version || !/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(version)) {
    throw new Error("Cargo.toml must contain a valid package version");
  }
  return version;
}

export function nextVersion(source: string, base: string): string {
  if (Bun.semver.order(source, base) > 0) return source;
  const [major, minor, patch] = base.split(/[.-]/);
  // Existing release policy reserves pre-1.0 patch releases for hotfixes.
  if (major === "0") return `0.${Number(minor) + 1}.0`;
  return `${major}.${minor}.${Number(patch) + 1}`;
}

// Called from the default-branch checkout. Candidate scripts are never executed
// with the release controller's write token.
export async function prepareRelease(root: string, base: string): Promise<string> {
  const manifest = join(root, "Cargo.toml");
  const source = packageVersion(readFileSync(manifest, "utf8"));
  const version = nextVersion(source, base);
  writeFileSync(manifest, readFileSync(manifest, "utf8").replace(
    /^version = "[^"]+"/m, `version = "${version}"`,
  ));
  const lock = join(root, "Cargo.lock");
  const locked = readFileSync(lock, "utf8");
  const updated = locked.replace(
    /(\[\[package\]\]\nname = "waku"\nversion = ")[^"]+(")/,
    (_, before, after) => `${before}${version}${after}`,
  );
  if (updated === locked && source !== version) throw new Error("waku package missing from Cargo.lock");
  writeFileSync(lock, updated);

  const hasFragments = [".changelog", ".changelog/mobile"].some((dir) => {
    const path = join(root, dir);
    return existsSync(path) && readdirSync(path).some((name) => /^(highlight|feat|exp|fix)-.*\.md$/.test(name));
  });
  if (hasFragments) {
    const process = Bun.spawn(["bun", resolve(import.meta.dir, "../../scripts/changelog.ts"), "collect"], {
      env: { ...globalThis.process.env, GODDARD_CHANGELOG_ROOT: root },
      stdout: "inherit", stderr: "inherit",
    });
    if (await process.exited !== 0) throw new Error("Changelog collection failed");
  }
  return version;
}

if (import.meta.main) {
  const [root, base] = process.argv.slice(2);
  if (!root || !base) throw new Error("Usage: prepare-release.ts ROOT BASE_VERSION");
  console.log(`RELEASE_VERSION=${await prepareRelease(resolve(root), base)}`);
}
