/** Snapshot commit and patch-diff helpers. */
import { createHash } from "node:crypto"
import { mkdtemp, rm } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"

import { git } from "./git.ts"
import type { RuntimeContext } from "./types.ts"

/** Captures tracked, modified, deleted, and untracked non-ignored files into one commit. */
export async function createSnapshotCommit(input: {
  cwd: string
  label: string
  context: RuntimeContext
}) {
  const scratchDir = await mkdtemp(join(tmpdir(), `review-sync-${process.pid}-`))
  const indexPath = join(
    scratchDir,
    `${createHash("sha256").update(input.cwd).digest("hex")}.index`,
  )

  try {
    await git(input.cwd, ["read-tree", "HEAD"], input.context, {
      env: { GIT_INDEX_FILE: indexPath },
    })
    await git(input.cwd, ["add", "-A", "--", "."], input.context, {
      env: { GIT_INDEX_FILE: indexPath },
    })
    const tree = (
      await git(input.cwd, ["write-tree"], input.context, {
        env: { GIT_INDEX_FILE: indexPath },
      })
    ).stdout.trim()
    const commit = await git(
      input.cwd,
      ["commit-tree", tree, "-p", "HEAD", "-m", `review-sync:${input.label}`],
      input.context,
      {
        env: {
          GIT_INDEX_FILE: indexPath,
          GIT_AUTHOR_NAME: "Review Sync",
          GIT_AUTHOR_EMAIL: "review-sync@local",
          GIT_COMMITTER_NAME: "Review Sync",
          GIT_COMMITTER_EMAIL: "review-sync@local",
        },
      },
    )
    return commit.stdout.trim()
  } finally {
    await rm(scratchDir, { recursive: true, force: true }).catch(() => {})
  }
}

/** Computes a binary Git diff between two snapshot commits. */
export async function diffCommits(
  cwd: string,
  before: string,
  after: string,
  context: RuntimeContext,
) {
  return (await git(cwd, ["diff", "--binary", before, after], context)).stdout
}
