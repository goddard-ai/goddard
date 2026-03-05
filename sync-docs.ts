#!/usr/bin/env tsx
/**
 * sync-docs.ts
 *
 * Fetch third-party documentation into docs/third_party/<repo-name>/.
 * Only *.md files (plus the .git/ bookkeeping folder) are kept after the clone.
 *
 * Usage:
 *   pnpm tsx sync-docs.ts <git-url> [subfolder]
 *
 * Examples:
 *   pnpm tsx sync-docs.ts https://github.com/drizzle-team/drizzle-orm
 *   pnpm tsx sync-docs.ts https://github.com/cloudflare/workers-sdk docs/workers
 */

import { execSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

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
  // Strip trailing .git, then take the last path segment.
  return url.replace(/\.git$/, "").split("/").filter(Boolean).at(-1)!;
}

/**
 * Recursively delete every file that is NOT a *.md file.
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
      if (entry.endsWith(".md")) {
        hasKeeper = true;
      } else {
        fs.rmSync(full, { force: true });
      }
    }
  }

  return hasKeeper;
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

const [, , gitUrl, subfolder] = process.argv;

if (!gitUrl) {
  console.error("Usage: pnpm tsx sync-docs.ts <git-url> [subfolder]");
  process.exit(1);
}

const repoName = repoNameFromUrl(gitUrl);
const targetBase = path.resolve("docs/third_party");
const targetDir = path.join(targetBase, repoName);

fs.mkdirSync(targetBase, { recursive: true });

// ---------------------------------------------------------------------------
// Clone or reset
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Declutter — strip everything that isn't a .md file
// ---------------------------------------------------------------------------

log(`Removing non-markdown files from ${targetDir}`);
declutter(targetDir);

log(`Done. Markdown files are available in docs/third_party/${repoName}/`);
