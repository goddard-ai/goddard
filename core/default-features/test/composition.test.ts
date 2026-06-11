import { expect, test } from "bun:test"

import { daemonIpcRoutes } from "../src/daemon-ipc.ts"
import { getDefaultDaemonPluginComposition } from "../src/daemon.ts"

test("default daemon composition includes file search", () => {
  expect(Object.hasOwn(daemonIpcRoutes, "fileSearch")).toBe(true)
  expect(
    getDefaultDaemonPluginComposition().plugins.some((plugin) => plugin.name === "file-search"),
  ).toBe(true)
})
