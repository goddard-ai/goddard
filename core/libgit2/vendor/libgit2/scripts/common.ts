import { spawn } from "node:child_process"
import { createHash } from "node:crypto"
import { access, mkdir, readdir, readFile, rm, stat, writeFile } from "node:fs/promises"
import { basename, dirname, join, resolve } from "node:path"
import { fileURLToPath } from "node:url"

import { nativeLibgit2Manifest } from "../manifest.ts"

export type TargetConfig =
  (typeof nativeLibgit2Manifest.targets)[keyof typeof nativeLibgit2Manifest.targets]

type Versions = {
  libgit2: {
    repo: string
    version: string
    ref: string
  }
}

export type CliOptions = {
  target: string
  json: boolean
}

export const rootDir = resolve(fileURLToPath(new URL("..", import.meta.url)))
export const sourceDir = join(rootDir, "source", "libgit2")
export const generatedRootNames = ["source", "build", "dist"] as const

export async function readVersions() {
  return JSON.parse(await readFile(join(rootDir, "versions.json"), "utf8")) as Versions
}

export async function resolveTarget(target: string) {
  const config = nativeLibgit2Manifest.targets[target as keyof typeof nativeLibgit2Manifest.targets]
  if (!config) {
    throw new Error(
      `Unsupported libgit2 target "${target}". Supported: ${Object.keys(nativeLibgit2Manifest.targets).join(", ")}`,
    )
  }
  return config
}

export function parseOptions(args = Bun.argv.slice(2)): CliOptions {
  let target = process.env.GODDARD_NATIVE_TARGET ?? defaultTarget()
  let json = false

  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index]
    if (arg === "--target") {
      const value = args[index + 1]
      if (!value) {
        throw new Error("--target requires a value")
      }
      target = value
      index += 1
    } else if (arg.startsWith("--target=")) {
      target = arg.slice("--target=".length)
    } else if (arg === "--json") {
      json = true
    } else {
      throw new Error(`Unknown argument: ${arg}`)
    }
  }

  return { target, json }
}

export function defaultTarget() {
  if (process.platform === "darwin" && process.arch === "arm64") {
    return "darwin-arm64"
  }
  return `${process.platform}-${process.arch}`
}

export function distDir(target: string) {
  return join(rootDir, "dist", target)
}

export function buildDir(target: string) {
  return join(rootDir, "build", target)
}

export async function artifactPath(target: string) {
  const config = await resolveTarget(target)
  return join(distDir(target), config.library)
}

export async function artifactManifestPath(target: string) {
  return join(distDir(target), "manifest.json")
}

export async function pathExists(path: string) {
  return await access(path).then(
    () => true,
    () => false,
  )
}

export async function ensureDir(path: string) {
  await mkdir(path, { recursive: true })
}

export async function removePath(path: string) {
  await rm(path, { recursive: true, force: true })
}

export async function run(command: string, args: string[], options: { cwd?: string } = {}) {
  const child = spawn(command, args, {
    cwd: options.cwd ?? rootDir,
    stdio: ["ignore", "pipe", "pipe"],
  })

  let stdout = ""
  let stderr = ""
  child.stdout.setEncoding("utf8")
  child.stderr.setEncoding("utf8")
  child.stdout.on("data", (chunk: string) => {
    stdout += chunk
  })
  child.stderr.on("data", (chunk: string) => {
    stderr += chunk
  })

  const status = await new Promise<number>((resolvePromise, rejectPromise) => {
    child.on("error", rejectPromise)
    child.on("close", (code) => resolvePromise(code ?? 1))
  })

  if (status !== 0) {
    throw new Error(
      `${command} ${args.join(" ")} failed in ${options.cwd ?? rootDir}\n${stderr || stdout}`,
    )
  }

  return { stdout, stderr }
}

export async function sha256(path: string) {
  const hash = createHash("sha256")
  hash.update(Buffer.from(await Bun.file(path).arrayBuffer()))
  return hash.digest("hex")
}

export async function writeJson(path: string, value: unknown) {
  await ensureDir(dirname(path))
  await writeFile(path, `${JSON.stringify(value, null, 2)}\n`)
}

export async function assertFile(path: string) {
  const info = await stat(path).catch(() => null)
  if (!info?.isFile()) {
    throw new Error(`Expected file to exist: ${path}`)
  }
}

export async function copyLicenseFiles(target: string) {
  const licenseDir = join(distDir(target), "licenses", "libgit2")
  await ensureDir(licenseDir)
  for (const entry of await readdir(sourceDir)) {
    if (/^(COPYING|LICENSE|AUTHORS)/i.test(entry)) {
      const from = join(sourceDir, entry)
      if ((await stat(from)).isFile()) {
        await Bun.write(join(licenseDir, basename(entry)), Bun.file(from))
      }
    }
  }
}

export async function printResult(value: unknown, json: boolean) {
  if (json) {
    console.log(JSON.stringify(value, null, 2))
    return
  }
  console.log(value)
}
