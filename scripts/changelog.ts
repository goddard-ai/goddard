// Pull a single version's notes out of a Keep-a-Changelog-style CHANGELOG.md.
//
// Release notes are written as fragments — one Markdown file per change under
// `.changelog/` — so parallel work never conflicts on CHANGELOG.md itself.
// `bun run changelog` folds every fragment into a `## [<version>]` section
// for the version in Cargo.toml, grouped by the filename's category prefix.
// A fragment may tag a topic group as a second filename segment —
// `feat-git-<slug>.md` — and grouped bullets nest under a `- **Group**`
// parent inside their `###` section. `bun scripts/changelog.ts check`
// previews the grouping without writing anything.
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
/** Fragments under `.changelog/mobile/` cover the mobile app and fold into
 *  CHANGELOG.mobile.md instead — the desktop changelog feeds the in-app
 *  updater prompt, so mobile-only notes must not land in it. */
const mobileFragmentsDir = join(fragmentsDir, "mobile");
const mobileChangelogPath = join(projectRoot, "CHANGELOG.mobile.md");
/** Highlight media is shared — `.changelog/media/<slug>.<ext>` backs a
 *  fragment in either directory. */
const mediaDir = join(fragmentsDir, "media");

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

/** Topic groups a non-highlight fragment can opt into via
 *  `<prefix>-<group>-<slug>.md`, in the order their `- **Group**`
 *  subsections are emitted: product surface first, infrastructure last.
 *  A group with fewer than MIN_GROUP_SIZE entries in a release folds
 *  back into the section's flat tail. */
const GROUPS = [
  { id: "sessions", label: "Sessions" },
  { id: "sidebar", label: "Sidebar" },
  { id: "composer", label: "Composer" },
  { id: "providers", label: "Providers" },
  { id: "git", label: "Git" },
  { id: "transcript", label: "Transcript" },
  { id: "panels", label: "Panels" },
  { id: "terminals", label: "Terminals" },
  { id: "keyboard", label: "Keyboard" },
  { id: "navigation", label: "Navigation" },
  { id: "appearance", label: "Appearance" },
  { id: "permissions", label: "Permissions" },
  { id: "settings", label: "Settings" },
  { id: "friends", label: "Friends" },
  { id: "ssh", label: "SSH" },
  { id: "platform", label: "Platform" },
] as const;

const MIN_GROUP_SIZE = 2;

/** Media extensions accepted for a highlight's screenshot or recording. */
const MEDIA_EXTS = ["png", "gif", "mp4", "mov"];

interface Fragment {
  name: string;
  heading: string;
  group: string | null;
  body: string;
}

/** Render one `###` section's fragments: grouped fragments nest under a
 *  `- **Group**` parent in GROUPS order, then ungrouped fragments and
 *  demoted singleton groups trail as flat bullets. */
function renderItems(items: Fragment[]): string {
  const grouped = new Map<string, Fragment[]>();
  const flat: Fragment[] = [];
  for (const item of items) {
    if (item.group) {
      const members = grouped.get(item.group) ?? [];
      members.push(item);
      grouped.set(item.group, members);
    } else {
      flat.push(item);
    }
  }

  const indent = (body: string) =>
    body
      .split("\n")
      .map((line) => (line ? `  ${line}` : line))
      .join("\n");

  const parts: string[] = [];
  for (const { id, label } of GROUPS) {
    const members = grouped.get(id);
    if (!members) continue;
    if (members.length < MIN_GROUP_SIZE) {
      flat.push(...members);
      continue;
    }
    parts.push(
      `- **${label}**\n${members.map((m) => indent(m.body)).join("\n")}`,
    );
  }
  flat.sort((a, b) => a.name.localeCompare(b.name));
  parts.push(...flat.map((m) => m.body));
  return parts.join("\n");
}

interface Release {
  version: string;
  desktop: Section | null;
  mobile: Section | null;
}

/** The rendered `## [<version>]` section for one changelog plus the
 *  fragments it consumed. `null` when the directory has no fragments. */
interface Section {
  heading: string;
  fragmentNames: string[];
  mediaMoves: [from: string, to: string][];
}

/** Read every `*.md` fragment directly inside `dir` and render the
 *  `## [<version>]` section they fold into. Returns null when the
 *  directory is absent or empty so a release with no mobile fragments
 *  leaves CHANGELOG.mobile.md untouched. */
async function buildSection(
  version: string,
  dir: string,
): Promise<Section | null> {
  const names = (existsSync(dir)
    ? readdirSync(dir, { withFileTypes: true })
    : [])
    .filter((entry) => entry.isFile() && entry.name.endsWith(".md"))
    .map((entry) => entry.name)
    .sort();
  if (names.length === 0) return null;

  const groups = new Map<string, Fragment[]>();
  for (const { heading } of CATEGORIES) groups.set(heading, []);
  const mediaMoves: [from: string, to: string][] = [];
  for (const name of names) {
    const category = CATEGORIES.find(({ prefix }) =>
      name.startsWith(prefix),
    );
    if (!category) {
      throw new Error(
        `Fragment ${name} has no category prefix; expected one of: ` +
          CATEGORIES.map(({ prefix }) => `${prefix}*`).join(", "),
      );
    }
    const rest = name.slice(category.prefix.length, -".md".length);
    // A recognized group token followed by a slug tags the fragment's
    // `**Group**` subsection; anything else leaves it ungrouped.
    // Highlights never group — their whole rest is the media slug.
    const [token, ...slugRest] = rest.split("-");
    const group =
      category.heading !== "Highlights" &&
      slugRest.length > 0 &&
      GROUPS.some(({ id }) => id === token)
        ? token
        : null;
    let body = (await Bun.file(join(dir, name)).text()).trim();

    if (category.heading === "Highlights") {
      const slug = rest;
      const media = MEDIA_EXTS.map((ext) => `${slug}.${ext}`).find((file) =>
        existsSync(join(mediaDir, file)),
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
      mediaMoves.push([join(mediaDir, media), join(projectRoot, releaseMedia)]);
    }

    if (category.heading === "Experiments") {
      body = body.replace(
        /^- (?:\*\*)?\[Experimental\](?:\*\*)?\s*/,
        "- ",
      );
      body = body.replace(/^- /, "- **[Experimental]** ");
    }

    if (body) {
      groups.get(category.heading)!.push({ name, heading: category.heading, group, body });
    }
  }

  const parts: string[] = [];
  for (const { heading } of CATEGORIES) {
    const items = groups.get(heading)!;
    if (items.length > 0) {
      parts.push(`### ${heading}\n\n${renderItems(items)}`);
    }
  }

  return {
    heading: `## [${version}]\n\n${parts.join("\n\n")}\n`,
    fragmentNames: names,
    mediaMoves,
  };
}

/** Read every fragment in both directories and render the `## [<version>]`
 *  sections they fold into. Shared by `collect` (which writes them) and
 *  `check` (which previews them). */
async function buildRelease(): Promise<Release> {
  const version = cargoVersion();
  const desktop = await buildSection(version, fragmentsDir);
  const mobile = await buildSection(version, mobileFragmentsDir);
  if (!desktop && !mobile) {
    throw new Error("Nothing to release: .changelog/ has no fragments.");
  }
  return { version, desktop, mobile };
}

const MOBILE_PREAMBLE =
  "# Changelog — Mobile\n\n" +
  "All notable changes to the Goddard mobile app. Fragments live under " +
  "`.changelog/mobile/` with the same naming rules as " +
  "[CHANGELOG.md](CHANGELOG.md) and fold here per release.\n";

/** Insert `section`'s `## [<version>]` block into the changelog text at
 *  `path`, above the first existing version section. Creates the file with
 *  `preamble` when it does not exist yet. */
async function foldSection(
  path: string,
  name: string,
  section: Section,
  version: string,
  preamble: string,
): Promise<void> {
  const file = Bun.file(path);
  const changelog = (await file.exists()) ? await file.text() : preamble;
  if (extractReleaseNotes(changelog, version) !== null) {
    throw new Error(`${name} already has a [${version}] section.`);
  }
  const lines = changelog.split("\n");
  const firstHeading = lines.findIndex((line) => /^##\s+/.test(line));
  const at = firstHeading === -1 ? lines.length : firstHeading;
  const out = [
    ...lines.slice(0, at),
    section.heading.trimEnd(),
    "",
    ...lines.slice(at),
  ].join("\n");
  await Bun.write(path, `${out.replace(/\n{3,}/g, "\n\n").trimEnd()}\n`);
}

/** Fold `.changelog/*.md` fragments into `## [<version>]` sections — desktop
 *  fragments into CHANGELOG.md, `.changelog/mobile/` into
 *  CHANGELOG.mobile.md — then delete the consumed fragments. */
export async function collectChangelog(): Promise<void> {
  const { version, desktop, mobile } = await buildRelease();
  if (desktop) {
    await foldSection(changelogPath, "CHANGELOG.md", desktop, version, "");
  }
  if (mobile) {
    await foldSection(
      mobileChangelogPath,
      "CHANGELOG.mobile.md",
      mobile,
      version,
      MOBILE_PREAMBLE,
    );
  }
  for (const name of desktop?.fragmentNames ?? []) {
    rmSync(join(fragmentsDir, name));
  }
  for (const name of mobile?.fragmentNames ?? []) {
    rmSync(join(mobileFragmentsDir, name));
  }
  for (const [from, to] of [
    ...(desktop?.mediaMoves ?? []),
    ...(mobile?.mediaMoves ?? []),
  ]) {
    mkdirSync(dirname(to), { recursive: true });
    renameSync(from, to);
  }
  console.log(
    `Collected ${desktop?.fragmentNames.length ?? 0} fragment(s) into ` +
      `[${version}] in CHANGELOG.md` +
      (mobile
        ? ` and ${mobile.fragmentNames.length} into CHANGELOG.mobile.md`
        : ""),
  );
}

/** Print the `## [<version>]` sections the current fragments would fold
 *  into, without writing the changelogs or consuming anything. */
async function checkChangelog(): Promise<void> {
  const { version, desktop, mobile } = await buildRelease();
  if (desktop) {
    console.log(
      `${desktop.fragmentNames.length} fragment(s) would collect into ` +
        `[${version}] in CHANGELOG.md:\n`,
    );
    console.log(desktop.heading);
  }
  if (mobile) {
    console.log(
      `${mobile.fragmentNames.length} fragment(s) would collect into ` +
        `[${version}] in CHANGELOG.mobile.md:\n`,
    );
    console.log(mobile.heading);
  }
}

if (import.meta.main) {
  const command = process.argv[2];
  if (command === "collect") {
    await collectChangelog();
  } else if (command === "check") {
    await checkChangelog();
  } else {
    console.error("usage: bun scripts/changelog.ts <collect|check>");
    process.exit(1);
  }
}
