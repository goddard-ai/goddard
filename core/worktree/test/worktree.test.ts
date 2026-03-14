import { describe, it, expect, vi, beforeEach } from "vitest"
import { Worktree } from "../src/index.ts"
import * as childProcess from "node:child_process"
import * as fs from "node:fs"

vi.mock("node:child_process", () => ({
  spawnSync: vi.fn(() => ({ status: 0, stdout: "" })),
}))

vi.mock("node:fs", () => ({
  existsSync: vi.fn(() => false),
  appendFileSync: vi.fn(),
}))

describe("Worktree", () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it("should create a worktree directory and branch name via copy fallback when worktrunk is missing", () => {
    vi.mocked(childProcess.spawnSync).mockImplementation((cmd, args) => {
      if (cmd === "wt" && args?.[0] === "--version") return { status: 1, stdout: "", error: undefined } as any
      return { status: 0, stdout: "", error: undefined } as any
    })
    vi.mocked(fs.existsSync).mockImplementation((p) => String(p).includes(".git") || String(p).includes(".worktrees"))

    const cwd = "/test/dir"
    const branchName = "pr-123"
    const worktree = new Worktree({ cwd })
    const result = worktree.setup(branchName)

    expect(worktree.plugin.name).toBe("default")
    expect(result.branchName).toBe("pr-123")
    expect(result.worktreeDir).toMatch(/^\/test\/dir\/.worktrees\/pr-123-\d+$/)

    expect(childProcess.spawnSync).toHaveBeenCalledWith(
      "cp",
      expect.any(Array),
      expect.objectContaining({ encoding: "utf8" })
    )
  })

  it("should throw if no .worktrees directory exists and no override is provided", () => {
    vi.mocked(childProcess.spawnSync).mockImplementation((cmd, args) => {
      if (cmd === "wt" && args?.[0] === "--version") return { status: 1, stdout: "", error: undefined } as any
      return { status: 0, stdout: "", error: undefined } as any
    })
    vi.mocked(fs.existsSync).mockImplementation((p) => String(p).includes(".git")) // Only .git exists

    const cwd = "/test/dir"
    const branchName = "pr-123"
    const worktree = new Worktree({ cwd })

    expect(() => worktree.setup(branchName)).toThrowError(/No default worktree directory found/)
  })

  it("should use a custom default directory if provided", () => {
    vi.mocked(childProcess.spawnSync).mockImplementation((cmd, args) => {
      if (cmd === "wt" && args?.[0] === "--version") return { status: 1, stdout: "", error: undefined } as any
      return { status: 0, stdout: "", error: undefined } as any
    })
    vi.mocked(fs.existsSync).mockImplementation((p) => String(p).includes(".git"))

    const cwd = "/test/dir"
    const branchName = "pr-123"
    const worktree = new Worktree({ cwd, defaultPluginDirName: ".custom-dir" })
    const result = worktree.setup(branchName)

    expect(result.worktreeDir).toMatch(/^\/test\/dir\/.custom-dir\/pr-123-\d+$/)
  })

  it("should handle git fetch and checkout errors gracefully", () => {
    // Mock git commands to fail, but cp and mkdir to succeed
    vi.mocked(childProcess.spawnSync).mockImplementation((cmd, args) => {
      if (cmd === "wt" && args?.[0] === "--version") return { status: 1, stdout: "", error: undefined } as any
      if (cmd === "git") return { status: 1, stdout: "", error: undefined } as any
      return { status: 0, stdout: "", error: undefined } as any
    })
    vi.mocked(fs.existsSync).mockImplementation((p) => String(p).includes(".git") || String(p).includes(".worktrees"))

    const cwd = "/test/dir"
    const branchName = "pr-123"
    const worktree = new Worktree({ cwd })

    // Should not throw
    expect(() => worktree.setup(branchName)).not.toThrow()
  })

  it("should use worktrunk if available", () => {
    vi.mocked(childProcess.spawnSync).mockImplementation((cmd, args) => {
      if (cmd === "wt" && args?.[0] === "--version") return { status: 0, stdout: "1.0.0", error: undefined } as any
      if (cmd === "wt" && args?.[0] === "switch") return { status: 0, stdout: "", error: undefined } as any
      if (cmd === "git" && args?.[0] === "worktree" && args?.[1] === "list") {
        return { status: 0, stdout: "/test/dir/.wt/pr-123 e1234 [pr-123]\n/test/dir main [main]", error: undefined } as any
      }
      return { status: 0, stdout: "", error: undefined } as any
    })
    vi.mocked(fs.existsSync).mockImplementation((p) => String(p).includes(".config/wt.toml") || String(p).includes(".git"))

    const cwd = "/test/dir"
    const branchName = "pr-123"
    const worktree = new Worktree({ cwd })
    const result = worktree.setup(branchName)

    expect(worktree.plugin.name).toBe("worktrunk")
    expect(worktree.poweredBy).toBe("worktrunk")
    expect(result.branchName).toBe("pr-123")
    expect(result.worktreeDir).toBe("/test/dir/.wt/pr-123")

    expect(childProcess.spawnSync).toHaveBeenCalledWith("wt", ["switch", "pr-123"], expect.any(Object))

    // Ensure it does NOT check out PR code manually since worktrunk handles it natively
    expect(childProcess.spawnSync).not.toHaveBeenCalledWith("git", ["fetch", "origin", "pull/123/head:pr-123"], expect.any(Object))
    expect(childProcess.spawnSync).not.toHaveBeenCalledWith("git", ["checkout", "pr-123"], expect.any(Object))
  })

  it("should dynamically fallback to default plugin if worktrunk setup returns null", () => {
    vi.mocked(childProcess.spawnSync).mockImplementation((cmd, args) => {
      if (cmd === "wt" && args?.[0] === "--version") return { status: 0, stdout: "1.0.0", error: undefined } as any

      // Simulate worktrunk switch failing, causing it to return null
      if (cmd === "wt" && args?.[0] === "switch") return { status: 1, stdout: "", error: undefined } as any

      return { status: 0, stdout: "", error: undefined } as any
    })
    vi.mocked(fs.existsSync).mockImplementation((p) => String(p).includes(".config/wt.toml") || String(p).includes(".git") || String(p).includes(".worktrees"))

    const cwd = "/test/dir"
    const branchName = "pr-123"
    const worktree = new Worktree({ cwd })

    // Should initially select worktrunk because it's applicable
    expect(worktree.plugin.name).toBe("worktrunk")
    expect(worktree.poweredBy).toBe("worktrunk")

    const result = worktree.setup(branchName)

    // Plugin should have been updated to the fallback due to failure
    expect(worktree.plugin.name).toBe("default")
    expect(worktree.poweredBy).toBe("default")
    expect(result.branchName).toBe("pr-123")
    expect(result.worktreeDir).toMatch(/^\/test\/dir\/.worktrees\/pr-123-\d+$/)

    // Check that fallback legacy copy commands were executed
    expect(childProcess.spawnSync).toHaveBeenCalledWith(
      "cp",
      expect.any(Array),
      expect.objectContaining({ encoding: "utf8" })
    )
  })
})

describe("cleanupWorktree", () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it("should use rm -rf for non-worktrunk directories", () => {
    vi.mocked(childProcess.spawnSync).mockImplementation((cmd, args) => {
      if (cmd === "wt" && args?.[0] === "--version") return { status: 1, stdout: "", error: undefined } as any
      return { status: 0, stdout: "", error: undefined } as any
    })
    vi.mocked(fs.existsSync).mockImplementation((p) => String(p).includes(".git"))

    const worktree = new Worktree({ cwd: "/test/dir" })
    worktree.cleanup("/test/dir/.goddard-agents/pr-123-1234", "pr-123")

    expect(childProcess.spawnSync).toHaveBeenCalledWith("rm", ["-rf", "/test/dir/.goddard-agents/pr-123-1234"], expect.any(Object))
  })

  it("should use wt remove if worktrunk plugin is active", () => {
    vi.mocked(childProcess.spawnSync).mockImplementation((cmd, args) => {
      if (cmd === "wt" && args?.[0] === "--version") return { status: 0, stdout: "1.0.0", error: undefined } as any
      if (cmd === "wt" && args?.[0] === "remove") return { status: 0, stdout: "", error: undefined } as any
      return { status: 0, stdout: "", error: undefined } as any
    })
    vi.mocked(fs.existsSync).mockImplementation((p) => String(p).includes(".config/wt.toml") || String(p).includes(".git"))

    const worktree = new Worktree({ cwd: "/test/dir" })
    worktree.cleanup("/test/dir/.wt/pr-123", "pr-123")

    expect(childProcess.spawnSync).toHaveBeenCalledWith("wt", ["remove", "pr-123"], expect.any(Object))
    expect(childProcess.spawnSync).not.toHaveBeenCalledWith("rm", ["-rf", "/test/dir/.wt/pr-123"], expect.any(Object))
  })
})
