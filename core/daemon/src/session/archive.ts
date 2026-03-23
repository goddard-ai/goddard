import { createWriteStream } from "node:fs"
import { access, chmod, mkdir, readdir, rm, stat } from "node:fs/promises"
import { createRequire } from "node:module"
import { dirname, isAbsolute, join, relative, resolve } from "node:path"
import { Readable } from "node:stream"
import { pipeline } from "node:stream/promises"
import { createGunzip } from "node:zlib"
import * as tarFs from "tar-fs"

const require = createRequire(import.meta.url)
const StreamZip: typeof import("node-stream-zip") = require("node-stream-zip")
const unbzip2Stream: typeof import("unbzip2-stream") = require("unbzip2-stream")

/** Marker file written after a binary payload has been fully installed into its cache directory. */
export const binaryInstallMarkerFileName = ".goddard-installed"

/** Supported remote payload formats for archive-backed ACP agent binaries. */
export type BinaryTargetPayloadFormat = "zip" | "tar.gz" | "tgz" | "tar.bz2" | "tbz2" | "raw"

/** Input contract for streaming one binary payload into a cache directory. */
export type InstallBinaryTargetPayloadInput = {
  archiveUrl: string
  cmd: string
  installDir: string
}

/** Detects how a binary payload should be unpacked based on its URL pathname. */
export function detectBinaryTargetPayloadFormat(archiveUrl: string): BinaryTargetPayloadFormat {
  const match = /\.(zip|tar\.gz|tgz|tar\.bz2|tbz2)$/i.exec(
    new URL(archiveUrl).pathname.toLowerCase(),
  )
  return (match?.[1] ?? "raw") as BinaryTargetPayloadFormat
}

/** Downloads one binary payload and unpacks it into the provided install directory. */
export async function installBinaryTargetPayload(
  input: InstallBinaryTargetPayloadInput,
): Promise<void> {
  const response = await fetch(input.archiveUrl)
  if (!response.ok) {
    throw new Error(
      `Failed to download binary archive ${input.archiveUrl}: ${response.status} ${response.statusText}`,
    )
  }
  if (!response.body) {
    throw new Error(`Binary archive response for ${input.archiveUrl} did not include a body`)
  }

  await mkdir(input.installDir, { recursive: true })

  const payloadStream = Readable.fromWeb(response.body as import("node:stream/web").ReadableStream)

  switch (detectBinaryTargetPayloadFormat(input.archiveUrl)) {
    case "raw":
      const binaryPath = resolveRawBinaryInstallPath(input.installDir, input.cmd)
      await mkdir(dirname(binaryPath), { recursive: true })
      await pipeline(payloadStream, createWriteStream(binaryPath))
      await chmod(binaryPath, 0o755)
      return
    case "tar.gz":
    case "tgz":
      await pipeline(payloadStream, createGunzip(), tarFs.extract(input.installDir))
      return
    case "tar.bz2":
    case "tbz2":
      await pipeline(payloadStream, unbzip2Stream(), tarFs.extract(input.installDir))
      return
    case "zip":
      await installZipPayload(payloadStream, input.installDir)
      await ensureInstalledCommandExecutable(input.installDir, input.cmd)
      return
    default:
      payloadStream.destroy()
      throw new Error(`Unsupported binary target payload format: ${input.archiveUrl}`)
  }
}

/** Stages one zip payload to disk and extracts it with `node-stream-zip`. */
async function installZipPayload(
  payloadStream: NodeJS.ReadableStream,
  installDir: string,
): Promise<void> {
  const zipPath = join(installDir, ".payload.zip")
  await pipeline(payloadStream, createWriteStream(zipPath))

  const zip = new StreamZip.async({ file: zipPath })
  try {
    await zip.extract(null, installDir)
  } finally {
    await zip.close()
    await rm(zipPath, { force: true })
  }
}

/** Restores executability for the declared command when the payload format does not preserve mode bits. */
async function ensureInstalledCommandExecutable(installDir: string, cmd: string): Promise<void> {
  if (process.platform === "win32" || isAbsolute(cmd)) {
    return
  }

  const commandPath = await resolveInstalledBinaryCommand(installDir, cmd)
  const commandStat = await stat(commandPath)
  await chmod(commandPath, commandStat.mode | 0o111)
}

/** Picks the extracted archive root that relative `cmd` paths should resolve against. */
export async function resolveInstalledBinaryRoot(installDir: string): Promise<string> {
  const entries = (await readdir(installDir, { withFileTypes: true })).filter(
    (entry) => entry.name !== binaryInstallMarkerFileName,
  )

  return entries.length === 1 && entries[0]?.isDirectory()
    ? join(installDir, entries[0].name)
    : installDir
}

/** Resolves a binary command path from the install root, falling back to a single extracted top-level directory when needed. */
export async function resolveInstalledBinaryCommand(
  installDir: string,
  cmd: string,
): Promise<string> {
  if (isAbsolute(cmd)) {
    return cmd
  }

  const directCommandPath = join(installDir, cmd)
  if (await pathExists(directCommandPath)) {
    return directCommandPath
  }

  const installRoot = await resolveInstalledBinaryRoot(installDir)
  return join(installRoot, cmd)
}

/** Maps a raw binary payload onto the relative command path it should occupy inside the install root. */
function resolveRawBinaryInstallPath(installDir: string, cmd: string): string {
  if (isAbsolute(cmd)) {
    throw new Error("Raw binary targets must declare a relative cmd path")
  }

  const installRoot = resolve(installDir)
  const binaryPath = resolve(installRoot, cmd)
  const relativePath = relative(installRoot, binaryPath)
  if (relativePath === "" || relativePath.startsWith("..") || isAbsolute(relativePath)) {
    throw new Error(`Raw binary command path must stay within the install directory: ${cmd}`)
  }

  return binaryPath
}

/** Checks whether a concrete installed command path exists on disk. */
async function pathExists(path: string): Promise<boolean> {
  try {
    await access(path)
    return true
  } catch {
    return false
  }
}
