import { spawnSync } from "node:child_process"

export function hasWorktrunk(): boolean {
  try {
    const result = spawnSync("wt", ["--version"])
    return result.status === 0
  } catch {
    return false
  }
}

export function setupWorktrunkWorktree(
  projectDir: string,
  prNumber: number,
  branchName: string,
): string | null {
  console.log(`\n[INFO] Worktrunk detected. Attempting to use it for PR workspace setup...`)
  try {
    const switchResult = spawnSync("wt", ["switch", `pr:${prNumber}`], {
      cwd: projectDir,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    })

    if (switchResult.status === 0) {
      // Find the newly created worktree path using git from within the project dir
      const worktreeListResult = spawnSync("git", ["worktree", "list"], {
        cwd: projectDir,
        encoding: "utf8",
      })

      if (worktreeListResult.status === 0) {
        const lines = worktreeListResult.stdout.split("\n")
        for (const line of lines) {
          if (line.includes(`[${branchName}]`)) {
            const wtPath = line.split(" ")[0]
            if (wtPath) {
              console.log(`[INFO] Successfully created and switched to Worktrunk workspace at ${wtPath}`)
              return wtPath
            }
          }
        }
      }
    }

    // Edge case: Worktrunk switch succeeded but we couldn't find the path.
    // Let's try to remove it before falling back so we don't leak it.
    if (switchResult.status === 0) {
      console.log(
        `[WARN] Worktrunk switch succeeded but failed to locate worktree path. Attempting to clean it up.`,
      )
      spawnSync("wt", ["remove", branchName], {
        cwd: projectDir,
        encoding: "utf8",
        stdio: "ignore",
      })
    }
  } catch {
    // Fallback
  }

  return null
}

export function cleanupWorktrunkWorktree(worktreeDir: string, branchName: string): boolean {
  try {
    const result = spawnSync("wt", ["remove", branchName], {
      // Execute command from parent dir so we aren't inside the directory we're trying to delete
      cwd: worktreeDir.split("/").slice(0, -1).join("/") || "/",
      encoding: "utf8",
      stdio: "ignore",
    })

    if (result.status === 0 && !result.error) {
      console.log(`[INFO] Successfully cleaned up Worktrunk workspace.`)
      return true
    }
    console.log(`[WARN] Worktrunk removal failed. Falling back to standard cleanup.`)
  } catch {
    console.log(`[WARN] Worktrunk removal exception. Falling back to standard cleanup.`)
  }

  return false
}
