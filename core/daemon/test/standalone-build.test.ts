import { createHash } from "node:crypto"
import { mkdir, mkdtemp, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join, relative } from "node:path"
import { afterAll, expect, test } from "bun:test"

import {
  listNativeLibraryFiles,
  stageNativeLibraries,
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
    target: "bun-darwin-arm64",
  })

  expect(nativeLibraries).toEqual({})
  expect(await listNativeLibraryFiles(outputDir, nativeLibraries)).toEqual([])
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
    target: "bun-darwin-arm64",
    libgit2SourceDir: sourceDir,
    libgit2Library: "libgit2.dylib",
    libgit2Version: "1.9.4",
  })

  expect(nativeLibraries.libgit2).toEqual({
    target: "bun-darwin-arm64",
    path: "native/libgit2/libgit2.dylib",
    version: "1.9.4",
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
    target: "bun-darwin-arm64",
    libgit2SourceDir: sourceDir,
    libgit2Library: "libgit2.dylib",
  })
  const secondHash = createHash("sha256")
  await updateNativeLibrariesHash(secondHash, outputDir, nativeLibraries)

  expect(secondHash.digest("hex")).not.toBe(firstHash.digest("hex"))
})

test("standalone native staging requires libgit2 source and library together", async () => {
  const outputDir = await createTempDir()

  await expect(
    stageNativeLibraries({
      outputDir,
      target: "bun-darwin-arm64",
      libgit2SourceDir: outputDir,
    }),
  ).rejects.toThrow("--native-libgit2-source-dir and --native-libgit2-library")
})

async function createTempDir() {
  const path = await mkdtemp(join(tmpdir(), "daemon-standalone-build-test-"))
  tempRoots.push(path)
  return path
}
