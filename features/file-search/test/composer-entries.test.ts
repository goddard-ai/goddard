import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join, resolve } from "node:path"
import { afterEach, describe, expect, mock, test } from "bun:test"

import { createFileSearchManager } from "../src/daemon/composer-entries.ts"

type MockFinder = {
  waitForScan: ReturnType<typeof mock>
  mixedSearch: ReturnType<typeof mock>
  destroy: ReturnType<typeof mock>
}

const tempDirectories: string[] = []

afterEach(async () => {
  await Promise.all(
    tempDirectories.splice(0).map((path) => rm(path, { recursive: true, force: true })),
  )
})

async function createTempProject() {
  const directory = await mkdtemp(join(tmpdir(), "goddard-file-search-"))
  tempDirectories.push(directory)
  return directory
}

function createMockFinder(items: unknown[] = []): MockFinder {
  return {
    waitForScan: mock(() => ({ ok: true, value: true })),
    mixedSearch: mock(() => ({
      ok: true,
      value: {
        items,
        scores: [],
        totalMatched: items.length,
        totalFiles: items.filter((item) => (item as { type?: string }).type === "file").length,
        totalDirs: items.filter((item) => (item as { type?: string }).type === "directory").length,
      },
    })),
    destroy: mock(),
  }
}

describe("file-search composer entries", () => {
  test("uses fff mixed search for an empty query", async () => {
    const cwd = await createTempProject()
    const finder = createMockFinder([
      {
        type: "directory",
        item: {
          relativePath: "src/",
        },
      },
      {
        type: "file",
        item: {
          relativePath: "README.md",
        },
      },
    ])
    const createFinder = mock(async () => ({ ok: true as const, value: finder }))
    const manager = createFileSearchManager({ createFinder })

    const result = await manager.composerEntries({
      cwd,
      query: "",
      limit: 10,
    })

    expect(createFinder).toHaveBeenCalledWith(resolve(cwd))
    expect(finder.mixedSearch).toHaveBeenCalledWith("", { pageSize: 10 })
    expect(
      result.entries.map(({ type, path, label, detail }) => ({ type, path, label, detail })),
    ).toEqual([
      {
        type: "folder",
        path: join(cwd, "src"),
        label: "src",
        detail: "./src",
      },
      {
        type: "file",
        path: join(cwd, "README.md"),
        label: "README.md",
        detail: "./README.md",
      },
    ])
  })

  test("maps fff mixed search results to composer entries", async () => {
    const cwd = await createTempProject()
    const finder = createMockFinder([
      {
        type: "directory",
        item: {
          relativePath: "src/components/",
        },
      },
      {
        type: "file",
        item: {
          relativePath: "src/index.ts",
        },
      },
    ])
    const createFinder = mock(async () => ({ ok: true as const, value: finder }))
    const manager = createFileSearchManager({ createFinder })

    const result = await manager.composerEntries({
      cwd,
      query: "src",
      limit: 10,
    })

    expect(createFinder).toHaveBeenCalledTimes(1)
    expect(createFinder).toHaveBeenCalledWith(resolve(cwd))
    expect(finder.mixedSearch).toHaveBeenCalledWith("src", { pageSize: 10 })
    expect(
      result.entries.map(({ type, path, label, detail }) => ({ type, path, label, detail })),
    ).toEqual([
      {
        type: "folder",
        path: join(cwd, "src/components"),
        label: "components",
        detail: "./src/components",
      },
      {
        type: "file",
        path: join(cwd, "src/index.ts"),
        label: "index.ts",
        detail: "./src/index.ts",
      },
    ])
  })

  test("reuses a finder for repeated non-empty queries in the same cwd", async () => {
    const cwd = await createTempProject()
    const finder = createMockFinder()
    const createFinder = mock(async () => ({ ok: true as const, value: finder }))
    const manager = createFileSearchManager({ createFinder })

    await manager.composerEntries({ cwd, query: "src", limit: 10 })
    await manager.composerEntries({ cwd, query: "test", limit: 10 })

    expect(createFinder).toHaveBeenCalledTimes(1)
    expect(finder.mixedSearch).toHaveBeenCalledTimes(2)
  })

  test("shares in-flight finder creation for concurrent queries in the same cwd", async () => {
    const cwd = await createTempProject()
    const finder = createMockFinder()
    const createCalls: string[] = []
    let resolveCreate: (result: { ok: true; value: MockFinder }) => void = () => undefined
    const createFinder = async (basePath: string) => {
      createCalls.push(basePath)
      return new Promise<{ ok: true; value: MockFinder }>((resolve) => {
        resolveCreate = resolve
      })
    }
    const manager = createFileSearchManager({ createFinder })

    const firstResult = manager.composerEntries({ cwd, query: "src", limit: 10 })
    const secondResult = manager.composerEntries({ cwd, query: "test", limit: 10 })

    resolveCreate({ ok: true, value: finder })
    await Promise.all([firstResult, secondResult])
    manager.destroy()

    expect(createCalls).toEqual([resolve(cwd)])
    expect(finder.mixedSearch).toHaveBeenCalledTimes(2)
    expect(finder.destroy).toHaveBeenCalledTimes(1)
  })

  test("falls back to recursive filesystem search when fff is unavailable", async () => {
    const cwd = await createTempProject()
    await mkdir(join(cwd, "src"))
    await mkdir(join(cwd, "node_modules"))
    await writeFile(join(cwd, "src", "composer.ts"), "")
    await writeFile(join(cwd, "node_modules", "composer.ts"), "")

    const manager = createFileSearchManager({
      createFinder: mock(async () => ({ ok: false as const, error: "native search unavailable" })),
    })

    const result = await manager.composerEntries({
      cwd,
      query: "composer",
      limit: 10,
    })

    expect(result.entries.map((entry) => entry.detail)).toEqual(["./src/composer.ts"])
  })

  test("applies the normalized result limit to fff searches", async () => {
    const cwd = await createTempProject()
    const finder = createMockFinder([
      { type: "file", item: { relativePath: "first.ts" } },
      { type: "file", item: { relativePath: "second.ts" } },
    ])
    const manager = createFileSearchManager({
      createFinder: mock(async () => ({ ok: true as const, value: finder })),
    })

    const result = await manager.composerEntries({
      cwd,
      query: "ts",
      limit: 1,
    })

    expect(finder.mixedSearch).toHaveBeenCalledWith("ts", { pageSize: 1 })
    expect(result.entries.map((entry) => entry.label)).toEqual(["first.ts"])
  })

  test("destroys idle finders before creating another finder", async () => {
    const firstCwd = await createTempProject()
    const secondCwd = await createTempProject()
    const firstFinder = createMockFinder()
    const secondFinder = createMockFinder()
    let currentTime = 0
    const createFinder = mock(async (basePath: string) => ({
      ok: true as const,
      value: basePath === resolve(firstCwd) ? firstFinder : secondFinder,
    }))
    const manager = createFileSearchManager({
      createFinder,
      now: () => currentTime,
    })

    await manager.composerEntries({ cwd: firstCwd, query: "src", limit: 10 })
    currentTime = 10 * 60 * 1000 + 1
    await manager.composerEntries({ cwd: secondCwd, query: "src", limit: 10 })

    expect(firstFinder.destroy).toHaveBeenCalledTimes(1)
    expect(secondFinder.destroy).not.toHaveBeenCalled()
  })

  test("destroys an in-flight finder that resolves after manager shutdown", async () => {
    const cwd = await createTempProject()
    const finder = createMockFinder()
    let resolveCreate: (result: { ok: true; value: MockFinder }) => void = () => undefined
    const manager = createFileSearchManager({
      createFinder: async () =>
        new Promise<{ ok: true; value: MockFinder }>((resolve) => {
          resolveCreate = resolve
        }),
    })

    const result = manager.composerEntries({ cwd, query: "src", limit: 10 })
    manager.destroy()
    resolveCreate({ ok: true, value: finder })
    await result

    expect(finder.destroy).toHaveBeenCalledTimes(1)
    expect(finder.mixedSearch).not.toHaveBeenCalled()
  })

  test("destroys cached finders on manager shutdown", async () => {
    const cwd = await createTempProject()
    const finder = createMockFinder()
    const manager = createFileSearchManager({
      createFinder: mock(async () => ({ ok: true as const, value: finder })),
    })

    await manager.composerEntries({ cwd, query: "src", limit: 10 })
    manager.destroy()

    expect(finder.destroy).toHaveBeenCalledTimes(1)
  })
})
