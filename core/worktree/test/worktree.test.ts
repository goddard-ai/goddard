import { beforeEach, describe, expect, it, vi } from "vitest"
import * as childProcess from "node:child_process"
import * as fs from "node:fs"
import * as os from "node:os"
import { Worktree } from "../src/index.ts"
import { defaultPlugin } from "../src/default-plugin.ts"
import type { WorktreePlugin } from "../src/types.ts"

vi.mock("node:child_process", (): typeof import("node:child_process") => ({
  spawnSync: vi.fn(() => ({ status: 0, stdout: "", error: undefined })),
} as any))

vi.mock("node:fs", (): typeof import("node:fs") => ({
  existsSync: vi.fn(() => false),
} as any))

function createPlugin(name: string, overrides: Partial<WorktreePlugin> = {}): WorktreePlugin {
  return {
    name,
    isApplicable: () => false,
    setup: () => null,
    cleanup: () => true,
    ...overrides,
  }
}

function mockRepo(paths: string[] = []) {
  vi.mocked(fs.existsSync).mockImplementation((value) => {
    const target = String(value)
    return (
      target.includes("/.git") ||
      target.endsWith(".git") ||
      paths.some((expectedPath) => target.includes(expectedPath))
    )
  })
}

function mockSpawnSync(
  implementation: (command: string, args: string[]) => { status: number; stdout?: string },
) {
  vi.mocked(childProcess.spawnSync).mockImplementation((command, args) => {
    return implementation(String(command), Array.isArray(args) ? args.map(String) : []) as never
  })
}

describe("Worktree", () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mockSpawnSync(() => ({ status: 0, stdout: "" }))
  })

  it("requires the cwd to be a git repository", () => {
    expect(() => new Worktree({ cwd: "/test/repo" })).toThrow("Not a git repository")
  })

  it("uses the first applicable custom plugin and delegates cleanup to it", () => {
    mockRepo()

    const setup = vi.fn(() => "/tmp/custom-worktree")
    const cleanup = vi.fn(() => true)
    const plugin = createPlugin("custom", {
      isApplicable: () => true,
      setup,
      cleanup,
    })

    const worktree = new Worktree({
      cwd: "/test/repo",
      plugins: [plugin, createPlugin("ignored", { isApplicable: () => true })],
    })

    expect(worktree.poweredBy).toBe("custom")
    expect(worktree.setup("feature-1")).toEqual({
      worktreeDir: "/tmp/custom-worktree",
      branchName: "feature-1",
    })

    worktree.cleanup("/tmp/custom-worktree", "feature-1")

    expect(setup).toHaveBeenCalledWith({
      cwd: "/test/repo",
      branchName: "feature-1",
      defaultDirName: undefined,
    })
    expect(cleanup).toHaveBeenCalledWith("/tmp/custom-worktree", "feature-1")
    expect(childProcess.spawnSync).not.toHaveBeenCalled()
  })

  it("falls back to the default plugin when the selected plugin returns null", () => {
    mockRepo(["/.worktrees"])

    const worktree = new Worktree({
      cwd: "/test/repo",
      plugins: [
        createPlugin("custom", {
          isApplicable: () => true,
          setup: () => null,
        }),
      ],
    })

    expect(worktree.poweredBy).toBe("custom")

    const result = worktree.setup("feature-1")

    expect(worktree.poweredBy).toBe("default")
    expect(result.branchName).toBe("feature-1")
    expect(result.worktreeDir).toMatch(/^\/test\/repo\/.worktrees\/feature-1-\d+$/)
  })

  it("falls back to the default plugin when worktrunk cannot set up the branch", () => {
    mockRepo([".config/wt.toml", "/.worktrees"])
    mockSpawnSync((command, args) => {
      if (command === "wt" && args[0] === "--version") {
        return { status: 0, stdout: "1.0.0" }
      }
      if (command === "wt" && args[0] === "switch") {
        return { status: 1, stdout: "" }
      }
      return { status: 0, stdout: "" }
    })

    const worktree = new Worktree({ cwd: "/test/repo" })

    expect(worktree.poweredBy).toBe("worktrunk")

    const result = worktree.setup("feature-1")

    expect(worktree.poweredBy).toBe("default")
    expect(result.worktreeDir).toMatch(/^\/test\/repo\/.worktrees\/feature-1-\d+$/)
  })

  it("uses worktrunk when it can resolve the created worktree", () => {
    mockRepo([".config/wt.toml"])
    mockSpawnSync((command, args) => {
      if (command === "wt" && args[0] === "--version") {
        return { status: 0, stdout: "1.0.0" }
      }
      if (command === "wt" && args[0] === "switch") {
        return { status: 0, stdout: "" }
      }
      if (command === "git" && args[0] === "worktree" && args[1] === "list") {
        return {
          status: 0,
          stdout: "/test/repo/.wt/feature-1 e1234 [feature-1]\n/test/repo main [main]",
        }
      }
      return { status: 0, stdout: "" }
    })

    const worktree = new Worktree({ cwd: "/test/repo" })
    const result = worktree.setup("feature-1")

    expect(worktree.poweredBy).toBe("worktrunk")
    expect(result).toEqual({
      worktreeDir: "/test/repo/.wt/feature-1",
      branchName: "feature-1",
    })
  })

  it("surfaces a contextual error when the default plugin cannot create a workspace", () => {
    mockRepo(["/.worktrees"])
    mockSpawnSync((command, args) => {
      if (command === "mkdir") {
        return { status: 0, stdout: "" }
      }
      if (command === "cp") {
        return { status: 1, stdout: "" }
      }
      if (command === "git" && args[0] === "worktree" && args[1] === "add") {
        return { status: 1, stdout: "" }
      }
      return { status: 0, stdout: "" }
    })

    const worktree = new Worktree({
      cwd: "/test/repo",
      plugins: [createPlugin("custom", { isApplicable: () => true, setup: () => null })],
    })

    expect(() => worktree.setup("feature-1")).toThrow(
      "Default worktree plugin failed to setup the workspace",
    )
  })
})

describe("defaultPlugin", () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mockSpawnSync(() => ({ status: 0, stdout: "" }))
  })

  it("uses an explicit default directory name when provided", () => {
    const result = defaultPlugin.setup({
      cwd: "/test/repo",
      branchName: "feature-1",
      defaultDirName: ".custom-dir",
    })

    expect(result).toMatch(/^\/test\/repo\/.custom-dir\/feature-1-\d+$/)
  })

  it("uses the shared global worktree directory when the repo has no local worktree dir", () => {
    vi.mocked(fs.existsSync).mockReturnValue(false)

    const result = defaultPlugin.setup({
      cwd: "/test/repo",
      branchName: "feature-1",
    })

    expect(result).toMatch(
      new RegExp(
        `^${os.homedir().replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}/.goddard/worktrees/repo-[a-f0-9]{7}/feature-1-\\d+$`,
      ),
    )
  })

  it("uses another workspace creation strategy when the first clone attempt fails", () => {
    const commands: string[] = []

    mockSpawnSync((command, args) => {
      commands.push([command, ...args].join(" "))

      if (command === "cp") {
        return { status: 1, stdout: "" }
      }
      if (command === "git" && args[0] === "worktree" && args[1] === "add") {
        return { status: 0, stdout: "" }
      }
      return { status: 0, stdout: "" }
    })

    const result = defaultPlugin.setup({
      cwd: "/test/repo",
      branchName: "feature-1",
      defaultDirName: ".worktrees",
    })

    expect(result).toMatch(/^\/test\/repo\/.worktrees\/feature-1-\d+$/)
    expect(commands.some((command) => command.startsWith("git worktree add"))).toBe(true)
  })

  it("falls back to removing the directory when git worktree cleanup fails", () => {
    const commands: string[] = []

    mockSpawnSync((command, args) => {
      commands.push([command, ...args].join(" "))

      if (command === "git" && args[0] === "worktree" && args[1] === "remove") {
        return { status: 1, stdout: "" }
      }
      return { status: 0, stdout: "" }
    })

    expect(defaultPlugin.cleanup("/test/repo/.worktrees/feature-1-1234", "feature-1")).toBe(true)
    expect(commands.some((command) => command.startsWith("rm -rf"))).toBe(true)
  })
})
