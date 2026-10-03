import type {
  AgentSession,
  ReviewDiffSource,
  ReviewEntry,
  UpstreamStatus,
} from "@waku/client";

export interface ReviewPatchFile {
  key: string;
  path: string;
  patch: string;
}

export function latestReviewTurnSource(
  session: AgentSession,
): ReviewDiffSource | null {
  for (let index = session.turns.length - 1; index >= 0; index -= 1) {
    const turn = session.turns[index]!;
    if (turn.turn_count > 0 && turn.checkpoint?.status === "ready") {
      return {
        lastTurn: {
          session_id: session.id,
          turn_id: turn.id,
          turn_count: turn.turn_count,
        },
      };
    }
  }
  return null;
}

export function reviewDiffSourceLabel(source: ReviewDiffSource): string {
  if (typeof source === "object") return `Turn ${source.lastTurn.turn_count}`;
  return {
    uncommitted: "Uncommitted",
    unstaged: "Unstaged",
    staged: "Staged",
    committed: "Committed",
    branch: "Branch",
    commit: "Commit",
  }[source];
}

/** Porcelain letter → short label for the git surface's file badges. */
export function gitStatusLabel(status: string, untracked: boolean): string {
  if (untracked || status === "??") return "new";
  return {
    M: "modified",
    A: "added",
    D: "deleted",
    R: "renamed",
    C: "copied",
    T: "typechange",
  }[status] ?? status.toLowerCase();
}

/** "origin/main · ↑2 ↓1" — the subtitle line under the branch name. */
export function upstreamLabel(upstream: UpstreamStatus | null): string | null {
  if (!upstream) return null;
  const parts = [upstream.name];
  if (upstream.ahead > 0) parts.push(`↑${upstream.ahead}`);
  if (upstream.behind > 0) parts.push(`↓${upstream.behind}`);
  if (upstream.ahead === 0 && upstream.behind === 0) parts.push("up to date");
  return parts.join(" · ");
}

/** Badge for a `qa`-queue commit — reverted and rejected outrank the
 * promotable flag since they explain why it can't land. */
export function reviewStatusLabel(entry: ReviewEntry): string {
  if (entry.reverted) return "reverted";
  if (entry.rejected) return "rejected";
  if (entry.approved) return entry.needsReview ? "approved" : "auto-approved";
  return "needs review";
}

const IMAGE_MIME: Record<string, string> = {
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
  heic: "image/heic",
  heif: "image/heif",
  avif: "image/avif",
  bmp: "image/bmp",
  svg: "image/svg+xml",
};

/** MIME for an image the Files viewer can render — `null` for anything
 * else, which falls back to the text viewer. */
export function imageMimeForPath(path: string): string | null {
  const ext = path.split(".").at(-1)?.toLowerCase() ?? "";
  return IMAGE_MIME[ext] ?? null;
}

export function parseNumstat(numstat: string): {
  files: number;
  additions: number;
  deletions: number;
} {
  let files = 0;
  let additions = 0;
  let deletions = 0;
  for (const line of numstat.trim().split("\n")) {
    if (!line) continue;
    const [added, removed] = line.split("\t");
    files += 1;
    additions += Number.parseInt(added ?? "", 10) || 0;
    deletions += Number.parseInt(removed ?? "", 10) || 0;
  }
  return { files, additions, deletions };
}

/** Split one Git patch into file-sized cards. The daemon already normalizes
 * the diff; this only recovers a stable display label for the compact mobile
 * review surface. */
export function splitReviewPatch(patch: string): ReviewPatchFile[] {
  const chunks = patch
    .split(/(?=^diff --git )/m)
    .map((chunk) => chunk.trim())
    .filter(Boolean);

  return chunks.map((chunk, index) => {
    const lines = chunk.split("\n");
    const path =
      diffMarkerPath(lines, "+++") ??
      diffMarkerPath(lines, "---") ??
      diffHeaderPath(lines) ??
      `Changed file ${index + 1}`;
    return { key: `${index}:${path}`, path, patch: chunk };
  });
}

function diffMarkerPath(lines: string[], marker: "+++" | "---"): string | null {
  const prefix = `${marker} `;
  const raw = lines
    .find((line) => line.startsWith(prefix))
    ?.slice(prefix.length)
    .trim();
  if (!raw || raw === "/dev/null") return null;
  return cleanDiffPath(raw);
}

function diffHeaderPath(lines: string[]): string | null {
  const header = lines.find((line) => line.startsWith("diff --git "));
  if (!header) return null;
  const match = header.match(/ b\/(.+)$/);
  return match?.[1] ? cleanDiffPath(match[1]) : null;
}

function cleanDiffPath(path: string): string {
  let value = path;
  if (value.startsWith('"') && value.endsWith('"')) {
    try {
      value = JSON.parse(value) as string;
    } catch {
      value = value.slice(1, -1);
    }
  }
  return value.replace(/^[ab]\//, "");
}
