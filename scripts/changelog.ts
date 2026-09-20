// Pull a single version's notes out of a Keep-a-Changelog-style CHANGELOG.md.
//
// Release notes are written as fragments — one Markdown file per change under
// `.changelog/` — so parallel work never conflicts on CHANGELOG.md itself.
// `bun run changelog` folds every fragment into a `## [<version>]` section
// for the version in Cargo.toml, grouped by the filename's category prefix.
import {
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  renameSync,
  rmSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";

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

/** Fragment categories, in the order their `###` sections are emitted.
 *  A fragment's filename must start with one of these prefixes. */
const CATEGORIES = [
  { prefix: "highlight-", heading: "Highlights" },
  { prefix: "feat-", heading: "Features" },
  { prefix: "exp-", heading: "Experiments" },
  { prefix: "fix-", heading: "Fixed" },
] as const;

/** Media extensions accepted for a highlight's screenshot or recording. */
const MEDIA_EXTS = ["png", "gif", "mp4", "mov"];

/** Fold `.changelog/*.md` fragments into a `## [<version>]` section grouped
 *  by category, then delete the consumed fragments. */
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

  const groups = new Map<string, string[]>();
  for (const { heading } of CATEGORIES) groups.set(heading, []);
  /** Highlight media to move into assets/release-notes/<version>/ on success. */
  const mediaMoves: [from: string, to: string][] = [];
  for (const name of fragments) {
    const category = CATEGORIES.find(({ prefix }) =>
      name.startsWith(prefix),
    );
    if (!category) {
      throw new Error(
        `Fragment ${name} has no category prefix; expected one of: ` +
          CATEGORIES.map(({ prefix }) => `${prefix}*`).join(", "),
      );
    }
    let body = (
      await Bun.file(join(fragmentsDir, name)).text()
    ).trim();

    if (category.heading === "Highlights") {
      const slug = name.slice(
        category.prefix.length,
        -".md".length,
      );
      const media = MEDIA_EXTS.map((ext) => `${slug}.${ext}`).find(
        (file) => existsSync(join(fragmentsDir, "media", file)),
      );
      if (!media) {
        throw new Error(
          `Fragment ${name} needs a screenshot or recording at ` +
            `.changelog/media/${slug}.{${MEDIA_EXTS.join(", ")}}`,
        );
      }
      if (!body.includes(`](media/${media})`)) {
        throw new Error(
          `Fragment ${name} must embed its media as ![](media/${media})`,
        );
      }
      const releaseMedia = `assets/release-notes/${version}/${media}`;
      body = body.replace(`](media/${media})`, `](${releaseMedia})`);
      mediaMoves.push([
        join(fragmentsDir, "media", media),
        join(projectRoot, releaseMedia),
      ]);
    }

    if (category.heading === "Experiments") {
      body = body.replace(
        /^- (?:\*\*)?\[Experimental\](?:\*\*)?\s*/,
        "- ",
      );
      body = body.replace(/^- /, "- **[Experimental]** ");
    }

    if (body) groups.get(category.heading)!.push(body);
  }

  const parts: string[] = [];
  for (const { heading } of CATEGORIES) {
    const items = groups.get(heading)!;
    if (items.length > 0) parts.push(`### ${heading}\n\n${items.join("\n")}`);
  }
  if (parts.length === 0) {
    throw new Error("Nothing to release: .changelog/ has no fragments.");
  }

  const section = `## [${version}]\n\n${parts.join("\n\n")}\n`;
  const lines = changelog.split("\n");
  const firstHeading = lines.findIndex((line) => /^##\s+/.test(line));
  const at = firstHeading === -1 ? lines.length : firstHeading;
  const out = [
    ...lines.slice(0, at),
    section.trimEnd(),
    "",
    ...lines.slice(at),
  ].join("\n");
  await Bun.write(
    changelogPath,
    `${out.replace(/\n{3,}/g, "\n\n").trimEnd()}\n`,
  );
  for (const name of fragments) rmSync(join(fragmentsDir, name));
  for (const [from, to] of mediaMoves) {
    mkdirSync(dirname(to), { recursive: true });
    renameSync(from, to);
  }
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
