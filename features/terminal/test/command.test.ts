import { expect, test } from "bun:test"

import { resolveTerminalLaunch } from "../src/daemon/command.ts"

test("Windows terminals use COMSPEC without POSIX login arguments", () => {
  expect(resolveTerminalLaunch({}, "win32", { COMSPEC: "C:\\Windows\\System32\\cmd.exe" })).toEqual(
    {
      command: "C:\\Windows\\System32\\cmd.exe",
      args: [],
    },
  )
  expect(resolveTerminalLaunch({}, "win32", {})).toEqual({
    command: "cmd.exe",
    args: [],
  })
  expect(resolveTerminalLaunch({ args: ["/Q"] }, "win32", { COMSPEC: "cmd.exe" })).toEqual({
    command: "cmd.exe",
    args: ["/Q"],
  })
})

test("POSIX terminals preserve shell and login defaults", () => {
  expect(resolveTerminalLaunch({}, "darwin", { SHELL: "/bin/zsh" })).toEqual({
    command: "/bin/zsh",
    args: ["-l"],
  })
  expect(resolveTerminalLaunch({}, "linux", {})).toEqual({
    command: "/bin/sh",
    args: ["-l"],
  })
})

test("explicit terminal commands and arguments override platform defaults", () => {
  expect(
    resolveTerminalLaunch({ command: "pwsh.exe", args: ["-NoLogo"] }, "win32", {
      COMSPEC: "cmd.exe",
    }),
  ).toEqual({
    command: "pwsh.exe",
    args: ["-NoLogo"],
  })
})
