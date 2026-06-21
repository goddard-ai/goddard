import { normalizePath } from "../paths.ts"
import type { WorktreeInfo } from "../types.ts"

export async function parseGitWorktrees(stdout: string) {
  const worktrees: WorktreeInfo[] = []
  let current: Partial<WorktreeInfo> = {}

  for (const line of stdout.split("\n")) {
    if (!line.trim()) {
      if (current.path) {
        worktrees.push({
          path: await normalizePath(current.path),
          branch: current.branch ?? null,
        })
      }
      current = {}
      continue
    }

    if (line.startsWith("worktree ")) {
      current.path = line.slice("worktree ".length)
    } else if (line.startsWith("branch refs/heads/")) {
      current.branch = line.slice("branch refs/heads/".length)
    }
  }

  if (current.path) {
    worktrees.push({
      path: await normalizePath(current.path),
      branch: current.branch ?? null,
    })
  }

  return worktrees
}

export function parseGitStashes(stdout: string) {
  return new Map(
    stdout
      .split("\n")
      .filter(Boolean)
      .map((line) => {
        const [ref, message = ""] = line.split("\0")
        return [ref, message] as const
      }),
  )
}
