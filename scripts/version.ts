// Shared Cargo version helpers for release packaging and the dev feed.

import { $ } from "bun";

type CargoMetadata = {
  packages: Array<{
    name: string;
    version: string;
  }>;
};

/** The workspace package's Cargo version. */
export async function cargoPackageVersion(
  projectRoot: string,
  packageName: string,
): Promise<string> {
  const metadata = JSON.parse(
    await $`cargo metadata --no-deps --format-version 1`
      .cwd(projectRoot)
      .quiet()
      .text(),
  ) as CargoMetadata;
  const cargoPackage = metadata.packages.find(
    (candidate) => candidate.name === packageName,
  );
  if (!cargoPackage) {
    throw new Error(`Cargo package "${packageName}" was not found.`);
  }
  return cargoPackage.version;
}

/** CFBundleVersion derived from the Cargo version. Sparkle decides which of
 *  two builds is newer by comparing this value, so it must grow with every
 *  release: three digits per semver field keep 0.2.0 → 2000 ahead of
 *  0.1.9 → 1009, and every release ahead of the pre-Sparkle DMGs that
 *  shipped CFBundleVersion 1. */
export function derivedBuildNumber(version: string): string {
  const match = version.match(/^(\d{1,3})\.(\d{1,3})\.(\d{1,3})(?:-|$)/);
  const major = Number(match?.[1]);
  const minor = Number(match?.[2]);
  const patch = Number(match?.[3]);
  if (![major, minor, patch].every(Number.isInteger)) {
    throw new Error(
      `Cannot derive a build number from version "${version}".`,
    );
  }
  return String(major * 1_000_000 + minor * 1_000 + patch);
}
