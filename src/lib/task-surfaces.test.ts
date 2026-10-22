import { describe, expect, test } from "bun:test";
import type { AgentSession, ReviewEntry } from "@waku/client";

import {
  gitStatusLabel,
  imageMimeForPath,
  latestReviewTurnSource,
  parseNumstat,
  reviewStatusLabel,
  splitReviewPatch,
  upstreamLabel,
} from "./task-surfaces";

describe("task surface presentation", () => {
  test("splits a multi-file review patch and keeps deleted file names", () => {
    const files = splitReviewPatch(`diff --git a/src/a.ts b/src/a.ts
--- a/src/a.ts
+++ b/src/a.ts
@@ -1 +1 @@
-old
+new
diff --git a/src/gone.ts b/src/gone.ts
--- a/src/gone.ts
+++ /dev/null
@@ -1 +0,0 @@
-gone`);

    expect(files.map((file) => file.path)).toEqual(["src/a.ts", "src/gone.ts"]);
    expect(files[0]!.patch).toContain("+new");
  });

  test("counts text numstat while treating binary markers as zero", () => {
    expect(parseNumstat("3\t1\tsrc/a.ts\n-\t-\timage.png")).toEqual({
      files: 2,
      additions: 3,
      deletions: 1,
    });
  });

  test("labels porcelain statuses for the git file badges", () => {
    expect(gitStatusLabel("M", false)).toBe("modified");
    expect(gitStatusLabel("D", false)).toBe("deleted");
    expect(gitStatusLabel("R", false)).toBe("renamed");
    expect(gitStatusLabel("??", true)).toBe("new");
    expect(gitStatusLabel("A", true)).toBe("new");
    expect(gitStatusLabel("U", false)).toBe("u");
  });

  test("summarizes upstream divergence in one line", () => {
    expect(upstreamLabel(null)).toBeNull();
    expect(upstreamLabel({ name: "origin/main", ahead: 0, behind: 0 })).toBe(
      "origin/main · up to date",
    );
    expect(upstreamLabel({ name: "origin/feat", ahead: 2, behind: 1 })).toBe(
      "origin/feat · ↑2 · ↓1",
    );
    expect(upstreamLabel({ name: "origin/feat", ahead: 3, behind: 0 })).toBe(
      "origin/feat · ↑3",
    );
  });

  test("labels image paths for the binary preview", () => {
    expect(imageMimeForPath("docs/logo.PNG")).toBe("image/png");
    expect(imageMimeForPath("icon.svg")).toBe("image/svg+xml");
    expect(imageMimeForPath("src/app.ts")).toBeNull();
    expect(imageMimeForPath("README")).toBeNull();
  });

  test("labels review-queue entries by the flag that blocks them", () => {
    const entry = (overrides: Partial<ReviewEntry>) =>
      ({
        commit: { sha: "x" },
        testPlans: [],
        needsReview: false,
        reviews: [],
        rejected: false,
        reverted: false,
        approved: false,
        ...overrides,
      }) as ReviewEntry;
    expect(reviewStatusLabel(entry({ reverted: true, approved: true }))).toBe(
      "reverted",
    );
    expect(reviewStatusLabel(entry({ rejected: true }))).toBe("rejected");
    expect(
      reviewStatusLabel(entry({ approved: true, needsReview: true })),
    ).toBe("approved");
    expect(reviewStatusLabel(entry({ approved: true }))).toBe("auto-approved");
    expect(reviewStatusLabel(entry({ needsReview: true }))).toBe(
      "needs review",
    );
  });

  test("selects the latest ready checkpoint for last-turn review", () => {
    const session = {
      id: "session",
      turns: [
        { id: "one", turn_count: 1, checkpoint: { status: "ready" } },
        { id: "two", turn_count: 2, checkpoint: { status: "failed" } },
      ],
    } as AgentSession;
    expect(latestReviewTurnSource(session)).toEqual({
      lastTurn: { session_id: "session", turn_id: "one", turn_count: 1 },
    });
  });
});
