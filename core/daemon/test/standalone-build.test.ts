import { createHash } from "node:crypto"
import { mkdir, mkdtemp, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join, relative } from "node:path"
import { afterAll, expect, test } from "bun:test"

import {
  listNativeLibraryFiles,
  resolveNativeLibgit2Target,
  stageNativeLibraries,
  stageSharedBunRuntime,
  updateNativeLibrariesHash,
} from "../scripts/build-standalone.ts"
import { removeTemporaryPath } from "./support/temp.ts"

const tempRoots: string[] = []

afterAll(async () => {
  while (tempRoots.length > 0) {
    await removeTemporaryPath(tempRoots.pop()!)
  }
})

test("standalone native manifest is absent when no native libraries are staged", async () => {
  const outputDir = await createTempDir()

  const nativeLibraries = await stageNativeLibraries({
    outputDir,
  })

  expect(nativeLibraries).toEqual({})
  expect(await listNativeLibraryFiles(outputDir, nativeLibraries)).toEqual([])
})

test("shared Bun staging preserves the complete bundled module graph", async () => {
  const rootDir = await createTempDir()
  const sourceDir = join(rootDir, "source")
  const outputDir = join(rootDir, "output")
  await mkdir(sourceDir, { recursive: true })
  await writeFile(
    join(sourceDir, "main.mjs"),
    'import { value } from "./chunk.mjs"\nconsole.log(value)\n',
  )
  await writeFile(join(sourceDir, "chunk.mjs"), "export const value = 1\n")

  const stagedFiles = await stageSharedBunRuntime({
    outputDir,
    entrypoints: [{ sourcePath: join(sourceDir, "main.mjs"), outputPath: "main.mjs" }],
  })

  expect(stagedFiles).toHaveLength(1)
  const result = Bun.spawnSync([process.execPath, stagedFiles[0]!])
  expect(result.exitCode).toBe(0)
  expect(result.stdout.toString()).toBe("1\n")
})

test("standalone native staging records libgit2 metadata and hashes staged files", async () => {
  const rootDir = await createTempDir()
  const sourceDir = join(rootDir, "source")
  const outputDir = join(rootDir, "output")
  await mkdir(join(sourceDir, "deps"), { recursive: true })
  await writeFile(join(sourceDir, "libgit2.dylib"), "libgit2-v1")
  await writeFile(join(sourceDir, "deps", "libssh2.dylib"), "dependency-v1")

  const nativeLibraries = await stageNativeLibraries({
    outputDir,
    libgit2: {
      target: "darwin-arm64",
      sourceDir,
      library: "libgit2.dylib",
      version: "1.9.0",
    },
  })

  expect(nativeLibraries.libgit2).toEqual({
    target: "darwin-arm64",
    path: "native/libgit2/libgit2.dylib",
    version: "1.9.0",
    sha256: createHash("sha256").update("libgit2-v1").digest("hex"),
  })
  expect(
    (await listNativeLibraryFiles(outputDir, nativeLibraries)).map((path) =>
      relative(outputDir, path).replaceAll("\\", "/"),
    ),
  ).toEqual(["native/libgit2/deps/libssh2.dylib", "native/libgit2/libgit2.dylib"])

  const firstHash = createHash("sha256")
  await updateNativeLibrariesHash(firstHash, outputDir, nativeLibraries)

  await writeFile(join(sourceDir, "deps", "libssh2.dylib"), "dependency-v2")
  await stageNativeLibraries({
    outputDir,
    libgit2: {
      target: "darwin-arm64",
      sourceDir,
      library: "libgit2.dylib",
      version: "1.9.0",
    },
  })
  const secondHash = createHash("sha256")
  await updateNativeLibrariesHash(secondHash, outputDir, nativeLibraries)

  expect(secondHash.digest("hex")).not.toBe(firstHash.digest("hex"))
})

test("standalone native staging rejects a library outside its artifact directory", async () => {
  const rootDir = await createTempDir()
  const outputDir = await createTempDir()

  await expect(
    stageNativeLibraries({
      outputDir,
      libgit2: {
        target: "darwin-arm64",
        sourceDir: join(rootDir, "artifact"),
        library: "../libgit2.dylib",
        version: "1.9.0",
      },
    }),
  ).rejects.toThrow("must be inside its artifact directory")
})

test("standalone builds resolve the package-owned libgit2 target", () => {
  expect(resolveNativeLibgit2Target("bun-darwin-arm64")).toBe("darwin-arm64")
  expect(() => resolveNativeLibgit2Target("bun-linux-x64")).toThrow(
    "No packaged libgit2 artifact supports bun-linux-x64",
  )
})

async function createTempDir() {
  const path = await mkdtemp(join(tmpdir(), "daemon-standalone-build-test-"))
  tempRoots.push(path)
  return path
}
