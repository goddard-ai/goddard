#!/usr/bin/env tsx
/**
 * sync-docs.ts
 *
 * Fetch third-party documentation into docs/third_party/<repo-name>/.
 * Only *.md and *.mdx files (plus the .git/ bookkeeping folder) are kept after the clone.
 *
 * Usage:
 *   pnpm sync-docs                          # sync all repos listed in synced_docs.json
 *   pnpm sync-docs <git-url> [subfolder]    # sync a single repo ad-hoc
 *
 * Examples:
 *   pnpm sync-docs
 *   pnpm sync-docs https://github.com/drizzle-team/drizzle-orm
 *   pnpm sync-docs https://github.com/cloudflare/workers-sdk docs
 *
 * Template syntax in synced_docs.json:
 *   {{dependencies.drizzle-orm}}              — version from root package.json
 *   {{devDependencies.tsx:backend}}           — version from backend/package.json
 *   Range specifiers (^, ~, >=, …) are stripped from resolved version strings.
 */

import { execSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

interface SyncedDocsEntry {
  url: string;
  name?: string;
  subfolder?: string;
  [key: string]: string | undefined;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function run(cmd: string, cwd?: string): string {
  return execSync(cmd, {
    cwd,
    encoding: "utf8",
    stdio: ["inherit", "pipe", "pipe"],
  }).trim();
}

function log(msg: string) {
  console.log(`[sync-docs] ${msg}`);
}

/** Derive a safe directory name from a git remote URL. */
function repoNameFromUrl(url: string): string {
  return url.replace(/\.git$/, "").split("/").filter(Boolean).at(-1)!;
}

/**
 * Recursively delete every file that is NOT a *.md or *.mdx file.
 * Directories are pruned if they become empty after the sweep.
 * The .git/ directory at the root is always preserved untouched.
 */
function declutter(dir: string, isRoot = true): boolean {
  let hasKeeper = false;

  for (const entry of fs.readdirSync(dir)) {
    const full = path.join(dir, entry);
    const stat = fs.statSync(full);

    if (stat.isDirectory()) {
      // Never touch the top-level .git folder.
      if (isRoot && entry === ".git") {
        hasKeeper = true;
        continue;
      }

      const subtreeHasKeeper = declutter(full, false);
      if (subtreeHasKeeper) {
        hasKeeper = true;
      } else {
        fs.rmSync(full, { recursive: true, force: true });
      }
    } else {
      if (entry.endsWith(".md") || entry.endsWith(".mdx")) {
        hasKeeper = true;
      } else {
        fs.rmSync(full, { force: true });
      }
    }
  }

  return hasKeeper;
}

// ---------------------------------------------------------------------------
// Template resolution
// ---------------------------------------------------------------------------

/** Cache of parsed package.json files keyed by their resolved absolute path. */
const pkgCache = new Map<string, Record<string, unknown>>();

function loadPackageJson(pkgPath: string): Record<string, unknown> {
  const abs = path.resolve(pkgPath);
  if (!pkgCache.has(abs)) {
    if (!fs.existsSync(abs)) {
      throw new Error(`package.json not found: ${abs}`);
    }
    pkgCache.set(abs, JSON.parse(fs.readFileSync(abs, "utf8")));
  }
  return pkgCache.get(abs)!;
}

/**
 * Resolve a single `{{token}}` expression.
 *
 * Token grammar:
 *   <field>.<packageName>              → root package.json
 *   <field>.<packageName>:<pkgDir>     → <pkgDir>/package.json
 *
 * Examples:
 *   dependencies.drizzle-orm
 *   devDependencies.vitest:backend
 */
function resolveToken(token: string): string {
  // Split on the first colon to get an optional package directory.
  const colonIdx = token.indexOf(":");
  const fieldKey = colonIdx === -1 ? token : token.slice(0, colonIdx);
  const pkgDir = colonIdx === -1 ? "." : token.slice(colonIdx + 1).trim();

  // fieldKey must be <field>.<packageName>
  const dotIdx = fieldKey.indexOf(".");
  if (dotIdx === -1) {
    throw new Error(
      `Invalid template token "{{${token}}}": expected "<field>.<packageName>" before the colon.`
    );
  }

  const field = fieldKey.slice(0, dotIdx).trim();
  const packageName = fieldKey.slice(dotIdx + 1).trim();

  const pkg = loadPackageJson(path.join(pkgDir, "package.json"));

  const fieldValue = pkg[field];
  if (!fieldValue || typeof fieldValue !== "object") {
    throw new Error(
      `Invalid template token "{{${token}}}": field "${field}" not found or not an object in ${path.join(pkgDir, "package.json")}.`
    );
  }

  const version = (fieldValue as Record<string, unknown>)[packageName];
  if (typeof version !== "string") {
    throw new Error(
      `Invalid template token "{{${token}}}": package "${packageName}" not found in "${field}" of ${path.join(pkgDir, "package.json")}.`
    );
  }

  // Strip semver range specifiers (^, ~, >=, >, <=, <, =) so the result is
  // a bare version string suitable for use in URLs or git refs.
  return version.replace(/^[~^>=<]+/, "");
}

/**
 * Replace all `{{…}}` tokens in a string using dependency versions from
 * package.json files.
 */
function resolveTemplates(value: string): string {
  return value.replace(/\{\{([^}]+)\}\}/g, (_, token: string) =>
    resolveToken(token.trim())
  );
}

/**
 * Apply template resolution to all string fields of a synced_docs.json entry.
 */
function resolveEntry(entry: SyncedDocsEntry): SyncedDocsEntry {
  return Object.fromEntries(
    Object.entries(entry).map(([k, v]) => [
      k,
      typeof v === "string" ? resolveTemplates(v) : v,
    ])
  ) as SyncedDocsEntry;
}

// ---------------------------------------------------------------------------
// Core sync logic
// ---------------------------------------------------------------------------

/**
 * Move the contents of `targetDir/subfolder/` up to `targetDir/`, then remove
 * the now-empty intermediate directory tree.
 *
 * This makes the subfolder the effective root of the cloned docs, so agents
 * see `docs/third_party/<name>/` rather than `docs/third_party/<name>/<subfolder>/`.
 *
 * Safe to call on subsequent reset runs: git restores the sparse-checkout state
 * (recreating the subfolder), and the hoist simply overwrites any leftover files
 * from the previous run before declutter tidies everything up.
 */
function hoistSubfolder(targetDir: string, subfolder: string): void {
  const subPath = path.join(targetDir, subfolder);
  if (!fs.existsSync(subPath)) return;

  // Stash the subfolder contents in a temp directory so we can wipe targetDir
  // cleanly (including any root-level files like README.md that git materialised
  // outside the sparse-checkout cone) before promoting them.
  const tmpDir = `${targetDir}_hoist_tmp`;
  fs.renameSync(subPath, tmpDir);

  // Remove everything in targetDir except .git/.
  for (const entry of fs.readdirSync(targetDir)) {
    if (entry === ".git") continue;
    fs.rmSync(path.join(targetDir, entry), { recursive: true, force: true });
  }

  // Move the stashed contents into the now-clean targetDir.
  for (const entry of fs.readdirSync(tmpDir)) {
    fs.renameSync(path.join(tmpDir, entry), path.join(targetDir, entry));
  }

  fs.rmSync(tmpDir, { recursive: true, force: true });
}

function syncRepo(gitUrl: string, subfolder?: string, name?: string): void {
  const repoName = name ?? repoNameFromUrl(gitUrl);
  const targetBase = path.resolve("docs/third_party");
  const targetDir = path.join(targetBase, repoName);

  fs.mkdirSync(targetBase, { recursive: true });

  if (!fs.existsSync(path.join(targetDir, ".git"))) {
    log(`Cloning ${gitUrl} → ${targetDir}`);

    if (subfolder) {
      // Sparse checkout — only materialise the requested subfolder.
      run(
        `git clone --filter=blob:none --no-checkout --depth 1 "${gitUrl}" "${targetDir}"`
      );
      run(`git sparse-checkout init --cone`, targetDir);
      run(`git sparse-checkout set "${subfolder}"`, targetDir);
      run(`git checkout`, targetDir);
    } else {
      run(`git clone --depth 1 "${gitUrl}" "${targetDir}"`);
    }
  } else {
    log(`Repo already cloned at ${targetDir} — resetting to origin`);

    // Make sure the remote is still correct (handles URL changes).
    run(`git remote set-url origin "${gitUrl}"`, targetDir);

    const branch = run(`git rev-parse --abbrev-ref HEAD`, targetDir);
    log(`Current branch: ${branch}`);

    run(`git fetch origin`, targetDir);
    run(`git reset --hard "origin/${branch}"`, targetDir);

    // Re-apply sparse checkout if a subfolder was requested.
    if (subfolder) {
      run(`git sparse-checkout init --cone`, targetDir);
      run(`git sparse-checkout set "${subfolder}"`, targetDir);
    }
  }

  if (subfolder) hoistSubfolder(targetDir, subfolder);
  log(`Removing non-markdown files from ${targetDir}`);
  declutter(targetDir);

  log(`Done. Markdown files are available in docs/third_party/${repoName}/`);
}

// ---------------------------------------------------------------------------
// Main — single ad-hoc repo or batch from synced_docs.json
// ---------------------------------------------------------------------------

const [, , gitUrl, subfolder] = process.argv;

if (gitUrl) {
  syncRepo(gitUrl, subfolder);
} else {
  const manifestPath = path.resolve("synced_docs.json");

  if (!fs.existsSync(manifestPath)) {
    console.error(
      `[sync-docs] No arguments provided and no synced_docs.json found at ${manifestPath}`
    );
    process.exit(1);
  }

  const rawEntries: SyncedDocsEntry[] = JSON.parse(
    fs.readFileSync(manifestPath, "utf8")
  );

  if (!Array.isArray(rawEntries) || rawEntries.length === 0) {
    log("synced_docs.json is empty — nothing to sync.");
    process.exit(0);
  }

  log(`Syncing ${rawEntries.length} repo(s) from synced_docs.json…`);

  for (const raw of rawEntries) {
    if (!raw.url) {
      console.warn(
        `[sync-docs] Skipping entry with missing "url": ${JSON.stringify(raw)}`
      );
      continue;
    }

    let entry: SyncedDocsEntry;
    try {
      entry = resolveEntry(raw);
    } catch (err) {
      console.error(
        `[sync-docs] Failed to resolve templates in entry ${JSON.stringify(raw)}:\n  ${(err as Error).message}`
      );
      process.exit(1);
    }

    log(`\n— ${entry.url}${entry.subfolder ? ` (${entry.subfolder})` : ""}`);
    syncRepo(entry.url, entry.subfolder, entry.name);
  }

  log("\nAll repos synced.");
}
