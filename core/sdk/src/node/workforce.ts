import { execFile } from "node:child_process"
import { createHash } from "node:crypto"
import { watch as watchFs, type FSWatcher, type Stats } from "node:fs"
import { mkdir, readFile, readdir, stat, writeFile } from "node:fs/promises"
import { basename, join, relative, resolve } from "node:path"
import { promisify } from "node:util"
import type { AgentSession } from "../daemon/session/client-session.ts"
import { runAgent, type RunAgentOptions } from "../daemon/session/client.ts"

const execFileAsync = promisify(execFile)

/** The name of the file within a workforce package's .goddard/ directory
 * that acts as the primary "inbox" for incoming agent requests. */
const REQUESTS_FILE = "requests.jsonl"

/** The name of the file within a workforce package's .goddard/ directory
 * where the agent records its own activity or internal feedback. */
const RESPONSES_FILE = "responses.jsonl"

/** A state file used to track the last processed byte offset and content
 * hash for the watched JSONL files to ensure idempotent processing. */
const PROCESSED_AT_FILE = "processed-at.json"

/** The set of files within a .goddard/ directory that the workforce
 * supervisor monitors for new appends. */
const WATCHED_FILES = [REQUESTS_FILE, RESPONSES_FILE] as const

/** Common directory names that should be skipped when recursively
 * discovering packages to avoid performance issues and noise. */
const IGNORED_DIRECTORY_NAMES = new Set([".git", "dist", "node_modules"])

/** The duration to wait after a file change is detected before syncing
 * state, allowing multiple rapid writes to be batched. */
const WATCH_SYNC_DELAY_MS = 25

// Package metadata discovered from nested package manifests under a
// repository root.
export type DiscoveredWorkforcePackage = {
  rootDir: string
  relativeDir: string
  manifestPath: string
  name: string
  goddardDir: string
  requestsPath: string
  responsesPath: string
}

// Result metadata returned after initializing a package's workforce files.
export type InitializedWorkforcePackage = {
  packageDir: string
  goddardDir: string
  createdPaths: string[]
}

// Persisted append-tracking state for one watched JSONL file.
export interface WorkforceProcessedFileState {
  offset: number
  prefixHash: string
  updatedAt: string
}

/** Package-local runtime state stored in `.goddard/processed-at.json`. */
export interface WorkforceProcessedState {
  /** Schema version for future-proofing the state object format. */
  version: 1

  /** A map of file names to their respective last-processed offset and hash state. */
  files: Partial<Record<WorkforceWatchedFileName, WorkforceProcessedFileState>>
}

/** File names supervised inside each workforce-enabled `.goddard` directory. */
export type WorkforceWatchedFileName = (typeof WATCHED_FILES)[number]

// Runtime event stream emitted by the workforce supervisor for host logging.
export type WorkforceRuntimeEvent =
  | {
      type: "package-discovered"
      package: DiscoveredWorkforcePackage
    }
  | {
      type: "session-started"
      package: DiscoveredWorkforcePackage
      sessionId: string
    }
  | {
      type: "batch-prompted"
      package: DiscoveredWorkforcePackage
      batchCount: number
      lineCount: number
    }
  | {
      type: "batch-queued"
      package: DiscoveredWorkforcePackage
      batchCount: number
      lineCount: number
    }
  | {
      type: "runtime-error"
      package: DiscoveredWorkforcePackage
      error: Error
    }

// Options accepted by the Node-only workforce watcher runtime.
export type WorkforceWatchOptions = {
  rootDir: string
  daemon?: RunAgentOptions
  onEvent?: (event: WorkforceRuntimeEvent) => void
}

/** One append-derived prompt batch associated with a specific watched file. */
interface WorkforcePromptBatch {
  /** The name of the file (e.g., requests.jsonl) that triggered this batch. */
  fileName: WorkforceWatchedFileName

  /** The full path to the file that was read. */
  filePath: string

  /** The newly appended UTF-8 content read from the file. */
  content: string
}

/** In-memory watcher runtime for a single package-scoped workforce session. */
interface WorkforcePackageRuntime {
  /** The immutable discovery metadata for this package. */
  package: DiscoveredWorkforcePackage

  /** The persistent AI session managing this package's domain. */
  session: AgentSession

  /** The FS watcher instance monitoring changes in the .goddard/ directory. */
  watcher: FSWatcher

  /** A timer handle for debouncing multiple rapid file changes. */
  syncTimer: NodeJS.Timeout | null

  /** Whether the supervisor has stopped this runtime. */
  stopped: boolean

  /** Whether an active prompt call is currently being awaited by this agent. */
  promptActive: boolean

  /** A queue of prompt batches that are pending transmission to the agent. */
  pendingBatches: WorkforcePromptBatch[]
}

function emitEvent(onEvent: WorkforceWatchOptions["onEvent"], event: WorkforceRuntimeEvent): void {
  try {
    onEvent?.(event)
  } catch (error) {
    console.error(error)
  }
}

async function resolvePackageName(manifestPath: string, packageDir: string): Promise<string> {
  try {
    const parsed = JSON.parse(await readFile(manifestPath, "utf-8")) as { name?: string }
    if (typeof parsed.name === "string" && parsed.name.trim()) {
      return parsed.name
    }
  } catch {
    // Fall back to the directory name when the manifest cannot be parsed.
  }

  return basename(packageDir)
}

async function walkDirectory(
  directory: string,
  visitor: (entryPath: string) => Promise<void>,
): Promise<void> {
  const entries = await readdir(directory, { withFileTypes: true })

  for (const entry of entries) {
    if (!entry.isDirectory()) {
      continue
    }

    if (IGNORED_DIRECTORY_NAMES.has(entry.name)) {
      continue
    }

    const entryPath = join(directory, entry.name)
    await visitor(entryPath)
    await walkDirectory(entryPath, visitor)
  }
}

async function discoverNestedPackageDirs(rootDir: string): Promise<string[]> {
  const resolvedRootDir = resolve(rootDir)
  const packageDirs: string[] = []

  try {
    const rootManifestStats = await stat(join(resolvedRootDir, "package.json"))
    if (rootManifestStats.isFile()) {
      packageDirs.push(resolvedRootDir)
    }
  } catch {
    // Ignore repositories without a root package manifest.
  }

  await walkDirectory(resolvedRootDir, async (entryPath) => {
    const manifestPath = join(entryPath, "package.json")
    try {
      const manifestStats = await stat(manifestPath)
      if (manifestStats.isFile()) {
        packageDirs.push(entryPath)
      }
    } catch {
      // Ignore directories that are not package roots.
    }
  })

  return packageDirs.sort()
}

async function toDiscoveredPackage(
  rootDir: string,
  packageDir: string,
): Promise<DiscoveredWorkforcePackage> {
  return {
    rootDir: packageDir,
    relativeDir: relative(rootDir, packageDir) || ".",
    manifestPath: join(packageDir, "package.json"),
    name: await resolvePackageName(join(packageDir, "package.json"), packageDir),
    goddardDir: join(packageDir, ".goddard"),
    requestsPath: join(packageDir, ".goddard", REQUESTS_FILE),
    responsesPath: join(packageDir, ".goddard", RESPONSES_FILE),
  }
}

function buildProcessedStatePath(pkg: DiscoveredWorkforcePackage): string {
  return join(pkg.goddardDir, PROCESSED_AT_FILE)
}

async function readProcessedState(
  pkg: DiscoveredWorkforcePackage,
): Promise<WorkforceProcessedState | null> {
  try {
    const parsed = JSON.parse(
      await readFile(buildProcessedStatePath(pkg), "utf-8"),
    ) as WorkforceProcessedState

    if (parsed.version !== 1 || typeof parsed.files !== "object" || parsed.files === null) {
      return null
    }

    return parsed
  } catch {
    return null
  }
}

async function writeProcessedState(
  pkg: DiscoveredWorkforcePackage,
  state: WorkforceProcessedState,
): Promise<void> {
  await mkdir(pkg.goddardDir, { recursive: true })
  await writeFile(buildProcessedStatePath(pkg), `${JSON.stringify(state, null, 2)}\n`, "utf-8")
}

function hashContent(content: string): string {
  return createHash("sha256").update(content).digest("hex")
}

function findLastCompleteOffset(content: string): number {
  if (!content) {
    return 0
  }

  if (content.endsWith("\n")) {
    return Buffer.byteLength(content)
  }

  const lastNewlineIndex = content.lastIndexOf("\n")
  if (lastNewlineIndex === -1) {
    return 0
  }

  return Buffer.byteLength(content.slice(0, lastNewlineIndex + 1))
}

async function readUtf8Prefix(path: string, offset: number): Promise<string> {
  if (offset <= 0) {
    return ""
  }

  return (await readFile(path)).subarray(0, offset).toString("utf-8")
}

async function createProgressEntry(
  path: string,
  offset: number,
): Promise<WorkforceProcessedFileState> {
  const prefix = await readUtf8Prefix(path, offset)

  return {
    offset,
    prefixHash: hashContent(prefix),
    updatedAt: new Date().toISOString(),
  }
}

async function buildSeedEntry(path: string): Promise<WorkforceProcessedFileState> {
  const content = await readFile(path, "utf-8")
  return createProgressEntry(path, findLastCompleteOffset(content))
}

async function ensureProgressState(
  pkg: DiscoveredWorkforcePackage,
  fileName: WorkforceWatchedFileName,
  filePath: string,
  currentState: WorkforceProcessedState | null,
): Promise<WorkforceProcessedState> {
  const state = currentState ?? { version: 1, files: {} }
  if (state.files[fileName]) {
    return state
  }

  try {
    state.files[fileName] = await buildSeedEntry(filePath)
  } catch {
    state.files[fileName] = {
      offset: 0,
      prefixHash: hashContent(""),
      updatedAt: new Date().toISOString(),
    }
  }

  await writeProcessedState(pkg, state)
  return state
}

async function syncFileStateForRewrite(
  path: string,
  stats: Stats,
): Promise<WorkforceProcessedFileState> {
  if (stats.size === 0) {
    return {
      offset: 0,
      prefixHash: hashContent(""),
      updatedAt: new Date().toISOString(),
    }
  }

  return buildSeedEntry(path)
}

async function readNewBatch(
  pkg: DiscoveredWorkforcePackage,
  fileName: WorkforceWatchedFileName,
  currentState: WorkforceProcessedState,
): Promise<WorkforcePromptBatch | null> {
  const filePath = join(pkg.goddardDir, fileName)
  let fileStats: Stats

  try {
    fileStats = await stat(filePath)
  } catch {
    return null
  }

  let fileState = currentState.files[fileName]
  if (!fileState) {
    currentState.files[fileName] = await syncFileStateForRewrite(filePath, fileStats)
    await writeProcessedState(pkg, currentState)
    return null
  }

  if (fileStats.size < fileState.offset) {
    currentState.files[fileName] = await syncFileStateForRewrite(filePath, fileStats)
    await writeProcessedState(pkg, currentState)
    return null
  }

  const persistedPrefix = await readUtf8Prefix(filePath, fileState.offset)
  if (hashContent(persistedPrefix) !== fileState.prefixHash) {
    currentState.files[fileName] = await syncFileStateForRewrite(filePath, fileStats)
    await writeProcessedState(pkg, currentState)
    return null
  }

  if (fileStats.size <= fileState.offset) {
    return null
  }

  const appendedContent = (await readFile(filePath)).subarray(fileState.offset).toString("utf-8")
  const completeOffset = findLastCompleteOffset(appendedContent)
  if (completeOffset === 0) {
    return null
  }

  const content = appendedContent.slice(0, completeOffset)
  currentState.files[fileName] = await createProgressEntry(
    filePath,
    fileState.offset + Buffer.byteLength(content),
  )
  await writeProcessedState(pkg, currentState)

  return { fileName, filePath, content }
}

function countJsonlLines(content: string): number {
  return content.split("\n").filter((line) => line.length > 0).length
}

function buildBatchPrompt(
  pkg: DiscoveredWorkforcePackage,
  batches: WorkforcePromptBatch[],
): string {
  return [
    `Workforce activity detected for package "${pkg.name}".`,
    `Package path: ${pkg.relativeDir}`,
    "",
    "Process the newly appended JSONL records for this package-scoped domain.",
    ...batches.flatMap((batch, index) => [
      "",
      `Batch ${index + 1}`,
      `Source: ${batch.fileName}`,
      `File: ${relative(pkg.rootDir, batch.filePath)}`,
      "```jsonl",
      batch.content.replace(/\n$/, ""),
      "```",
    ]),
  ].join("\n")
}

function buildWorkforceSystemPrompt(pkg: DiscoveredWorkforcePackage): string {
  if (pkg.relativeDir === ".") {
    return [
      "You are the workforce root agent for this repository.",
      "You own the project-wide view, break work down, and route requests to package agents by appending to their .goddard/requests.jsonl files.",
      "Use the repository root as your domain unless delegating to subpackages.",
      "Treat .goddard/requests.jsonl and .goddard/responses.jsonl as asynchronous inbox files.",
    ].join("\n")
  }

  return [
    "You are the workforce lead agent for this package.",
    `Own only the code inside ${pkg.relativeDir}.`,
    "Treat .goddard/requests.jsonl and .goddard/responses.jsonl as asynchronous inbox files.",
    "Act on new appended records for this package-scoped domain and preserve clear boundaries with other packages.",
  ].join("\n")
}

async function drainPackageQueue(
  runtime: WorkforcePackageRuntime,
  onEvent: WorkforceWatchOptions["onEvent"],
): Promise<void> {
  if (runtime.stopped || runtime.promptActive || runtime.pendingBatches.length === 0) {
    return
  }

  runtime.promptActive = true
  const queuedBatches = runtime.pendingBatches.splice(0)

  try {
    await runtime.session.prompt(buildBatchPrompt(runtime.package, queuedBatches))
    emitEvent(onEvent, {
      type: "batch-prompted",
      package: runtime.package,
      batchCount: queuedBatches.length,
      lineCount: queuedBatches.reduce((count, batch) => count + countJsonlLines(batch.content), 0),
    })
  } catch (error) {
    runtime.pendingBatches = [...queuedBatches, ...runtime.pendingBatches]
    emitEvent(onEvent, {
      type: "runtime-error",
      package: runtime.package,
      error: error instanceof Error ? error : new Error(String(error)),
    })
  } finally {
    runtime.promptActive = false
  }

  if (!runtime.stopped && runtime.pendingBatches.length > 0) {
    emitEvent(onEvent, {
      type: "batch-queued",
      package: runtime.package,
      batchCount: runtime.pendingBatches.length,
      lineCount: runtime.pendingBatches.reduce(
        (count, batch) => count + countJsonlLines(batch.content),
        0,
      ),
    })
    void drainPackageQueue(runtime, onEvent)
  }
}

async function syncPackageFiles(
  runtime: WorkforcePackageRuntime,
  onEvent: WorkforceWatchOptions["onEvent"],
): Promise<void> {
  if (runtime.stopped) {
    return
  }

  let currentState = await readProcessedState(runtime.package)

  for (const fileName of WATCHED_FILES) {
    currentState = await ensureProgressState(
      runtime.package,
      fileName,
      join(runtime.package.goddardDir, fileName),
      currentState,
    )

    const batch = await readNewBatch(runtime.package, fileName, currentState)
    if (batch?.content) {
      runtime.pendingBatches.push(batch)
    }
  }

  if (runtime.pendingBatches.length > 0) {
    void drainPackageQueue(runtime, onEvent)
  }
}

function schedulePackageSync(
  runtime: WorkforcePackageRuntime,
  onEvent: WorkforceWatchOptions["onEvent"],
): void {
  if (runtime.stopped || runtime.syncTimer) {
    return
  }

  runtime.syncTimer = setTimeout(() => {
    runtime.syncTimer = null
    void syncPackageFiles(runtime, onEvent)
  }, WATCH_SYNC_DELAY_MS)
}

async function startPackageRuntime(
  pkg: DiscoveredWorkforcePackage,
  options: WorkforceWatchOptions,
): Promise<WorkforcePackageRuntime> {
  emitEvent(options.onEvent, { type: "package-discovered", package: pkg })

  const session = await runAgent(
    {
      agent: "pi",
      cwd: pkg.rootDir,
      mcpServers: [],
      systemPrompt: buildWorkforceSystemPrompt(pkg),
    },
    undefined,
    options.daemon,
  )

  if (!session) {
    throw new Error(`Persistent workforce sessions must not resolve to null for ${pkg.relativeDir}`)
  }

  emitEvent(options.onEvent, {
    type: "session-started",
    package: pkg,
    sessionId: session.sessionId,
  })

  const runtime: WorkforcePackageRuntime = {
    package: pkg,
    session,
    watcher: watchFs(pkg.goddardDir, () => {
      schedulePackageSync(runtime, options.onEvent)
    }),
    syncTimer: null,
    stopped: false,
    promptActive: false,
    pendingBatches: [],
  }

  runtime.watcher.on("error", (error) => {
    emitEvent(options.onEvent, {
      type: "runtime-error",
      package: pkg,
      error: error instanceof Error ? error : new Error(String(error)),
    })
  })

  await syncPackageFiles(runtime, options.onEvent)
  return runtime
}

/** A supervisor class that manages multiple workforce agents and their underlying filesystem watches. */
export class WorkforceSupervisor {
  /** A collection of active runtimes, one for each discovered workforce package. */
  readonly #runtimes: WorkforcePackageRuntime[]

  constructor(runtimes: WorkforcePackageRuntime[]) {
    this.#runtimes = runtimes
  }

  /** Gracefully stops all agent sessions and filesystem watchers. */
  async stop(): Promise<void> {
    await Promise.all(
      this.#runtimes.map(async (runtime) => {
        runtime.stopped = true
        if (runtime.syncTimer) {
          clearTimeout(runtime.syncTimer)
          runtime.syncTimer = null
        }

        runtime.watcher.close()
        await runtime.session.stop().catch(() => {})
      }),
    )
  }
}

export async function resolveRepositoryRoot(startDir: string): Promise<string> {
  try {
    const { stdout } = await execFileAsync("git", ["rev-parse", "--show-toplevel"], {
      cwd: resolve(startDir),
    })
    return stdout.trim()
  } catch (error) {
    throw new Error(
      `Unable to resolve the repository root from ${resolve(startDir)}: ${
        error instanceof Error ? error.message : String(error)
      }`,
    )
  }
}

export async function discoverWorkforcePackages(
  rootDir: string,
): Promise<DiscoveredWorkforcePackage[]> {
  const packages = await Promise.all(
    (await discoverNestedPackageDirs(rootDir)).map(async (packageDir) =>
      toDiscoveredPackage(rootDir, packageDir),
    ),
  )

  const discovered: DiscoveredWorkforcePackage[] = []

  for (const pkg of packages) {
    try {
      const goddardStats = await stat(pkg.goddardDir)
      if (goddardStats.isDirectory()) {
        discovered.push(pkg)
      }
    } catch {
      // Ignore packages without workforce state.
    }
  }

  return discovered
}

export async function discoverWorkforceInitCandidates(
  rootDir: string,
): Promise<DiscoveredWorkforcePackage[]> {
  const packages = await Promise.all(
    (await discoverNestedPackageDirs(rootDir)).map(async (packageDir) =>
      toDiscoveredPackage(rootDir, packageDir),
    ),
  )

  const candidates: DiscoveredWorkforcePackage[] = []

  for (const pkg of packages) {
    try {
      const requestsStats = await stat(pkg.requestsPath)
      if (requestsStats.isFile()) {
        continue
      }
    } catch {
      candidates.push(pkg)
    }
  }

  return candidates
}

export async function initializeWorkforcePackages(
  packageDirs: string[],
): Promise<InitializedWorkforcePackage[]> {
  const initialized: InitializedWorkforcePackage[] = []

  for (const packageDir of packageDirs) {
    const goddardDir = join(packageDir, ".goddard")
    const createdPaths: string[] = []

    await mkdir(goddardDir, { recursive: true })

    for (const fileName of WATCHED_FILES) {
      const filePath = join(goddardDir, fileName)
      try {
        const existing = await stat(filePath)
        if (!existing.isFile()) {
          await writeFile(filePath, "", "utf-8")
          createdPaths.push(filePath)
        }
      } catch {
        await writeFile(filePath, "", "utf-8")
        createdPaths.push(filePath)
      }
    }

    initialized.push({
      packageDir,
      goddardDir,
      createdPaths,
    })
  }

  return initialized
}

export async function watchWorkforce(options: WorkforceWatchOptions): Promise<WorkforceSupervisor> {
  const runtimes: WorkforcePackageRuntime[] = []

  try {
    for (const pkg of await discoverWorkforcePackages(options.rootDir)) {
      runtimes.push(await startPackageRuntime(pkg, options))
    }
  } catch (error) {
    await Promise.all(runtimes.map(async (runtime) => runtime.session.stop().catch(() => {})))
    throw error
  }

  return new WorkforceSupervisor(runtimes)
}
