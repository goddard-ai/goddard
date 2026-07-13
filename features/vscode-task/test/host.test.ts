import { describe, expect, test } from "bun:test"

import { resolveTaskTerminalOptions } from "../src/daemon/host.ts"

const baseRequest = {
  label: "build",
  cwd: "/workspace",
  env: { BUILD_MODE: "test" },
}

describe("workspace task terminal realization", () => {
  test("spawns process tasks directly", () => {
    expect(
      resolveTaskTerminalOptions(
        {
          ...baseRequest,
          kind: "process",
          command: "/usr/bin/node",
          args: ["build.mjs", "--watch"],
        },
        "linux",
      ),
    ).toEqual({
      command: "/usr/bin/node",
      args: ["build.mjs", "--watch"],
      cwd: "/workspace",
      env: { BUILD_MODE: "test" },
      title: "build",
    })
  })

  test("runs shell tasks through an explicit shell", () => {
    expect(
      resolveTaskTerminalOptions(
        {
          ...baseRequest,
          kind: "shell",
          command: "printf",
          args: ["hello world", "it's-ready"],
          shell: {
            executable: "/bin/bash",
            args: ["--noprofile", "-c"],
          },
        },
        "linux",
      ),
    ).toEqual({
      command: "/bin/bash",
      args: ["--noprofile", "-c", `printf 'hello world' 'it'"'"'s-ready'`],
      cwd: "/workspace",
      env: { BUILD_MODE: "test" },
      title: "build",
    })
  })

  test("uses cmd defaults and Windows quoting for Windows shell tasks", () => {
    const options = resolveTaskTerminalOptions(
      {
        ...baseRequest,
        kind: "shell",
        command: "echo",
        args: ["hello world"],
      },
      "windows",
    )

    expect(options.command).toBe(process.env.COMSPEC ?? "cmd.exe")
    expect(options.args).toEqual(["/d", "/s", "/c", 'echo "hello world"'])
  })

  test("uses the task environment when selecting a default shell", () => {
    const options = resolveTaskTerminalOptions(
      {
        ...baseRequest,
        env: { ...baseRequest.env, SHELL: "/bin/zsh" },
        kind: "shell",
        command: "echo ready",
        args: [],
      },
      "osx",
    )

    expect(options.command).toBe("/bin/zsh")
    expect(options.args).toEqual(["-c", "echo ready"])
  })
})
