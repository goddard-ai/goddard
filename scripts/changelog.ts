// Pull a single version's notes out of a Keep-a-Changelog-style CHANGELOG.md.
//
// Release notes are written as fragments — one Markdown file per change under
// `.changelog/` — so parallel work never conflicts on CHANGELOG.md itself.
// `bun run changelog` folds every fragment plus any `## [unreleased]` bullets
// into a `## [<version>]` section for the version in Cargo.toml.
import { existsSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { join, resolve } from "node:path";

/** The version token from a level-2 heading, or null if it isn't one.
 *  Handles `## [0.2.0] - 2026-08-08`, `## 0.2.0`, `## v0.2.0`, etc. */
function headingVersion(line: string): string | null {
  const match = line.match(/^##\s+(.+)$/); // level 2 only — `### …` won't match
  const token = match?.[1]?.trim().split(/\s+/)[0]; // before any ` - date`
  if (!token) return null;
  return token
    .replace(/^\[|\]$/g, "") // strip [ ]
    .replace(/^v/i, ""); // strip a leading v
}

/** Return the notes body for `version` (without its heading), or null if the
 *  changelog has no section for it. */
export function extractReleaseNotes(
  changelog: string,
  version: string,
): string | null {
  const lines = changelog.split("\n");

  let start = -1;
  for (let i = 0; i < lines.length; i++) {
    if (headingVersion(lines[i] ?? "") === version) {
      start = i + 1;
      break;
    }
  }
  if (start === -1) return null;

  let end = lines.length;
  for (let i = start; i < lines.length; i++) {
    if (/^##\s+/.test(lines[i] ?? "")) {
      end = i;
      break;
    }
  }

  const body = lines.slice(start, end).join("\n").trim();
  return body || null;
}

const projectRoot = resolve(import.meta.dir, "..");
const fragmentsDir = join(projectRoot, ".changelog");
const changelogPath = join(projectRoot, "CHANGELOG.md");

/** The workspace version — Cargo.toml is the single source of truth. */
function cargoVersion(): string {
  const text = readFileSync(join(projectRoot, "Cargo.toml"), "utf8");
  const version = text.match(/^version = "([^"]+)"/m)?.[1];
  if (!version) throw new Error("No version found in Cargo.toml");
  return version;
}

/** Fold `.changelog/*.md` fragments (and any `## [unreleased]` bullets) into a
 *  `## [<version>]` section, then delete the consumed fragments. */
export async function collectChangelog(): Promise<void> {
  const version = cargoVersion();
  const changelog = await Bun.file(changelogPath).text();
  if (extractReleaseNotes(changelog, version) !== null) {
    throw new Error(`CHANGELOG.md already has a [${version}] section.`);
  }

  const fragments = (existsSync(fragmentsDir)
    ? readdirSync(fragmentsDir, { withFileTypes: true })
    : [])
    .filter((entry) => entry.isFile() && entry.name.endsWith(".md"))
    .map((entry) => entry.name)
    .sort();
  const bodies: string[] = [];
  for (const name of fragments) {
    const body = (
      await Bun.file(join(fragmentsDir, name)).text()
    ).trim();
    if (body) bodies.push(body);
  }
  const unreleased = extractReleaseNotes(changelog, "unreleased");
  if (unreleased) bodies.unshift(unreleased);
  if (bodies.length === 0) {
    throw new Error(
      "Nothing to release: .changelog/ has no fragments and " +
        "## [unreleased] is empty.",
    );
  }

  const section = `## [${version}]\n\n${bodies.join("\n")}\n`;
  const lines = changelog.split("\n");
  const unreleasedIdx = lines.findIndex((line) =>
    /^##\s+\[?unreleased\]?/i.test(line),
  );
  const afterUnreleased =
    unreleasedIdx === -1
      ? -1
      : lines.findIndex(
          (line, index) => index > unreleasedIdx && /^##\s+/.test(line),
        );
  const cut = afterUnreleased === -1 ? lines.length : afterUnreleased;

  let out: string;
  if (unreleasedIdx === -1) {
    // No staging section: insert before the first version heading.
    const firstHeading = lines.findIndex((line) => /^##\s+/.test(line));
    const at = firstHeading === -1 ? lines.length : firstHeading;
    out = [
      ...lines.slice(0, at),
      section.trimEnd(),
      "",
      ...lines.slice(at),
    ].join("\n");
  } else {
    out = [
      ...lines.slice(0, unreleasedIdx + 1),
      "",
      section.trimEnd(),
      "",
      ...lines.slice(cut),
    ].join("\n");
  }
  await Bun.write(
    changelogPath,
    `${out.replace(/\n{3,}/g, "\n\n").trimEnd()}\n`,
  );
  for (const name of fragments) rmSync(join(fragmentsDir, name));
  console.log(
    `Collected ${fragments.length} fragment(s) into [${version}] in CHANGELOG.md`,
  );
}

if (import.meta.main) {
  const command = process.argv[2];
  if (command !== "collect") {
    console.error("usage: bun scripts/changelog.ts collect");
    process.exit(1);
  }
  await collectChangelog();
}
