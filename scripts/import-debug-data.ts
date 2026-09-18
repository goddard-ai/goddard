#!/usr/bin/env bun

/**
 * Adopts debug-build app data into the release data directory. Debug builds
 * never touch `~/Library/Application Support`: `StateStore::default_path`
 * keeps the whole data directory in the checkout's gitignored `temp/`
 * (overridable per-instance with `GODDARD_DATA_DIR`). The source is therefore
 * `$GODDARD_DATA_DIR`, then this checkout's `temp/`, then the primary
 * checkout's `temp/` (a worktree build still points at the checkout that
 * compiled it).
 *
 * - `state.json` and `composer-drafts.json` are copied only when absent.
 * - `app.json` and legacy `settings.json` are copied into `~/.goddard` —
 *   where release desktop settings live — only when absent.
 * - `blobs/` and `attachments/` entries are hardlinked only when absent
 *   (shared with the debug install instead of duplicated), falling back to
 *   a copy when the two directories live on different volumes.
 * - `app.db` is cloned through `VACUUM INTO` only when the destination has no
 *   database and every migration tag the source records is one this checkout
 *   ships (`db/migrations/*.sql` file stems) — the same gate the app applies,
 *   so a debug database from a newer lineage is left alone.
 * - Everything else (`model-cache/`, `commands/`, `shell-integration/`,
 *   `usage-model-rates.json`, `app.db` sidecars, unknown items) is skipped —
 *   they are caches or materialized artifacts the app regenerates. Sources
 *   are never removed.
 *
 * Existing destination items always win; to replace one, delete it first.
 */

import { Database } from "bun:sqlite";
import {
  existsSync,
  lstatSync,
  mkdirSync,
  readdirSync,
  renameSync,
  rmSync,
} from "node:fs";
import { copyFile, link, lstat, mkdir, readdir, realpath } from "node:fs/promises";
import { homedir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { createInterface } from "node:readline/promises";

const projectRoot = resolve(import.meta.dir, "..");
const applicationSupport = join(homedir(), "Library", "Application Support");
const destination = join(applicationSupport, "Goddard");
const settingsDestination = join(homedir(), ".goddard");

// Copied only when absent. State lives beside `app.db`; desktop settings
// (`app.json`, and the legacy `settings.json` the app migrates it from) live
// in the shared `~/.goddard` configuration directory.
const COPIED_STATE_FILES = new Set(["state.json", "composer-drafts.json"]);
const COPIED_SETTINGS_FILES = new Set(["app.json", "settings.json"]);
const LINKED_TREES = new Set(["attachments", "blobs"]);

type Report = {
  adopted: string[];
  skipped: string[];
  failures: Array<{ path: string; error: unknown }>;
};

const report: Report = { adopted: [], skipped: [], failures: [] };

function knownMigrationTags(): Set<string> {
  const directory = join(projectRoot, "db", "migrations");
  return new Set(
    readdirSync(directory)
      .filter((name) => name.endsWith(".sql"))
      .map((name) => basename(name, ".sql")),
  );
}

function databaseIsCompatible(path: string, tags: Set<string>): boolean | null {
  // null = could not open; false = predates or diverges from this schema.
  let db: Database;
  try {
    db = new Database(path, { readonly: true });
  } catch {
    return null;
  }
  try {
    const hasTable = db
      .query(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'migrations') AS e",
      )
      .get() as { e: number };
    if (!hasTable.e) return false;
    const recorded = db.query("SELECT tag FROM migrations").all() as Array<{
      tag: string;
    }>;
    return recorded.every(({ tag }) => tags.has(tag));
  } finally {
    db.close();
  }
}

async function copyFileIfAbsent(source: string, destination: string) {
  if (existsSync(destination)) {
    report.skipped.push(destination);
    return;
  }
  try {
    mkdirSync(dirname(destination), { recursive: true });
    await copyFile(source, destination);
    report.adopted.push(destination);
  } catch (error) {
    report.failures.push({ path: destination, error });
  }
}

async function linkTreeEntries(source: string, destination: string) {
  let entries;
  try {
    entries = await readdir(source, { withFileTypes: true });
  } catch (error) {
    report.failures.push({ path: source, error });
    return;
  }
  for (const entry of entries) {
    const sourceItem = join(source, entry.name);
    const destinationItem = join(destination, entry.name);
    if (entry.isDirectory()) {
      if (!existsSync(destinationItem)) await mkdir(destinationItem, { recursive: true });
      await linkTreeEntries(sourceItem, destinationItem);
    } else if (existsSync(destinationItem)) {
      report.skipped.push(destinationItem);
    } else {
      try {
        await link(sourceItem, destinationItem);
        report.adopted.push(destinationItem);
      } catch (error) {
        const code = (error as NodeJS.ErrnoException).code;
        // Hardlinks cannot cross volumes (EXDEV) and some filesystems reject
        // them outright (EPERM/ENOTSUP); copy instead so adoption still works.
        if (code === "EXDEV" || code === "EPERM" || code === "ENOTSUP") {
          try {
            await copyFile(sourceItem, destinationItem);
            report.adopted.push(destinationItem);
          } catch (copyError) {
            report.failures.push({ path: destinationItem, error: copyError });
          }
        } else {
          report.failures.push({ path: destinationItem, error });
        }
      }
    }
  }
}

async function adoptDatabase(source: string, destination: string) {
  if (existsSync(destination)) {
    report.skipped.push(destination);
    return;
  }
  const compatible = databaseIsCompatible(source, knownMigrationTags());
  if (compatible === null) {
    report.failures.push({
      path: source,
      error: new Error("could not open source database"),
    });
    return;
  }
  if (!compatible) {
    console.log(
      `Skipping ${source}: it predates the migration framework or records ` +
        "migrations this checkout does not ship.",
    );
    report.skipped.push(source);
    return;
  }
  const temp = `${destination}.import-${process.pid}`;
  rmSync(temp, { force: true });
  try {
    const db = new Database(source, { readonly: true });
    try {
      db.exec(`VACUUM INTO '${temp.replaceAll("'", "''")}'`);
    } finally {
      db.close();
    }
    renameSync(temp, destination);
    report.adopted.push(destination);
  } catch (error) {
    rmSync(temp, { force: true });
    report.failures.push({ path: destination, error });
  }
}

// Pick the debug source the way the app does: `GODDARD_DATA_DIR` first, then
// this checkout's `temp/`. A worktree checkout may not have built the running
// app, so fall back to the primary checkout's `temp/`.
function debugDataDirectory(): string {
  const override = process.env.GODDARD_DATA_DIR;
  if (override) return override;
  const local = join(projectRoot, "temp");
  if (existsSync(local)) return local;
  const common = Bun.spawnSync(
    ["git", "rev-parse", "--path-format=absolute", "--git-common-dir"],
    { cwd: projectRoot, stdout: "pipe", stderr: "ignore" },
  );
  if (common.exitCode === 0) {
    const primary = join(common.stdout.toString().trim(), "..", "temp");
    if (existsSync(primary)) return primary;
  }
  return local;
}

const source = debugDataDirectory();

if (!existsSync(source) || !lstatSync(source).isDirectory()) {
  console.log(
    `No debug data directory found at ${source}; nothing to import.\n` +
      "Debug builds keep their data in the checkout's `temp/` — set " +
      "GODDARD_DATA_DIR or run from the checkout that built the app.",
  );
  process.exit(0);
}

const running = [
  "Goddard",
  "Goddard Debug",
  "goddard-daemon",
  "goddard-debug-daemon",
].filter(
  (name) =>
    Bun.spawnSync(["/usr/bin/pgrep", "-x", name], {
      stdout: "ignore",
      stderr: "ignore",
    }).exitCode === 0,
);
if (running.length > 0) {
  console.warn(
    `Warning: ${running.join(", ")} is running. Quit the app and daemon ` +
      "first — the release app only adopts data at startup, and a running " +
      "process may overwrite what this imports.",
  );
}

console.log(
  `Importing ${source}\n  into ${destination}\n  and ${settingsDestination}\n`,
);

const readline = createInterface({ input: process.stdin, output: process.stdout });
let answer: string;
try {
  answer = (await readline.question("Proceed? [Y/n] ")).trim().toLowerCase();
} finally {
  readline.close();
}
if (answer !== "" && answer !== "y" && answer !== "yes") {
  console.log("Cancelled; nothing was imported.");
  process.exit(0);
}

mkdirSync(destination, { recursive: true });
const sourceRoot = await realpath(source);
for (const entry of await readdir(sourceRoot)) {
  const sourceItem = join(sourceRoot, entry);
  const destinationItem = join(destination, entry);
  const metadata = await lstat(sourceItem);
  if (entry === "app.db" && metadata.isFile()) {
    await adoptDatabase(sourceItem, destinationItem);
  } else if (COPIED_STATE_FILES.has(entry) && metadata.isFile()) {
    await copyFileIfAbsent(sourceItem, destinationItem);
  } else if (COPIED_SETTINGS_FILES.has(entry) && metadata.isFile()) {
    await copyFileIfAbsent(sourceItem, join(settingsDestination, entry));
  } else if (LINKED_TREES.has(entry) && metadata.isDirectory()) {
    await linkTreeEntries(sourceItem, destinationItem);
  } else {
    report.skipped.push(sourceItem);
  }
}

console.log(`\nAdopted ${report.adopted.length} item(s).`);
for (const path of report.adopted) console.log(`  + ${path}`);
if (report.skipped.length > 0) {
  console.log(`Skipped ${report.skipped.length} item(s) (already present or unrecognized).`);
}
if (report.failures.length > 0) {
  for (const { path, error } of report.failures) {
    console.error(`  ! ${path}:`, error);
  }
  process.exit(1);
}
