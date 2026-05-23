import { existsSync, statSync } from "node:fs"
import { homedir, platform } from "node:os"
import { join } from "node:path"
import { getGoddardUserDir } from "@goddard-ai/paths/node"
import { z } from "zod"

import type {
  DesktopPlatform,
  OpenWorktreeResponse,
  WorktreeOpener,
  WorktreeOpenerId,
  WorktreeOpenerKind,
  WorktreeOpenErrorCode,
  WorktreeOpenersResponse,
} from "~/shared/worktree-openers.ts"
import { daemonSend } from "./daemon.ts"
import { readJsonFile, writeJsonFile } from "./json-file.ts"

type NativeLaunchStrategy =
  | { type: "macos-open-app"; appBundlePath: string }
  | { type: "macos-finder" }
  | { type: "windows-explorer" }
  | { type: "linux-open-location" }

type NativeWorktreeOpener = WorktreeOpener & {
  launchStrategy: NativeLaunchStrategy
  discoveredPath?: string
}

type SessionWorktreeResponse = {
  worktree: { worktreeDir: string } | null
}

type SessionGetResponse = {
  session: { cwd: string }
}

const preferencesFileVersion = 1
const preferencesPath = join(getGoddardUserDir(), "worktree-openers.json")

const WorktreeOpenerPreferencesFile = z.strictObject({
  version: z.literal(preferencesFileVersion),
  lastUsedOpenerId: z.string().optional(),
})

const preferredOrder: WorktreeOpenerId[] = [
  "cursor",
  "vscode",
  "windsurf",
  "zed",
  "xcode",
  "android-studio",
  "webstorm",
  "vscode-insiders",
  "intellij-idea",
  "pycharm",
  "goland",
  "phpstorm",
  "sublime-text",
  "ghostty",
  "terminal",
  "finder",
  "explorer",
  "linux-files",
]

const macosApps: Array<{
  id: WorktreeOpenerId
  displayName: string
  kind: WorktreeOpenerKind
  bundleNames: string[]
}> = [
  { id: "cursor", displayName: "Cursor", kind: "ide", bundleNames: ["Cursor.app"] },
  {
    id: "vscode",
    displayName: "Visual Studio Code",
    kind: "ide",
    bundleNames: ["Visual Studio Code.app"],
  },
  {
    id: "vscode-insiders",
    displayName: "Visual Studio Code Insiders",
    kind: "ide",
    bundleNames: ["Visual Studio Code - Insiders.app"],
  },
  { id: "windsurf", displayName: "Windsurf", kind: "ide", bundleNames: ["Windsurf.app"] },
  { id: "zed", displayName: "Zed", kind: "ide", bundleNames: ["Zed.app"] },
  { id: "xcode", displayName: "Xcode", kind: "ide", bundleNames: ["Xcode.app"] },
  {
    id: "android-studio",
    displayName: "Android Studio",
    kind: "ide",
    bundleNames: ["Android Studio.app"],
  },
  {
    id: "webstorm",
    displayName: "WebStorm",
    kind: "ide",
    bundleNames: ["WebStorm.app"],
  },
  {
    id: "intellij-idea",
    displayName: "IntelliJ IDEA",
    kind: "ide",
    bundleNames: ["IntelliJ IDEA.app", "IntelliJ IDEA Ultimate.app"],
  },
  { id: "pycharm", displayName: "PyCharm", kind: "ide", bundleNames: ["PyCharm.app"] },
  { id: "goland", displayName: "GoLand", kind: "ide", bundleNames: ["GoLand.app"] },
  { id: "phpstorm", displayName: "PhpStorm", kind: "ide", bundleNames: ["PhpStorm.app"] },
  {
    id: "sublime-text",
    displayName: "Sublime Text",
    kind: "ide",
    bundleNames: ["Sublime Text.app"],
  },
  { id: "ghostty", displayName: "Ghostty", kind: "terminal", bundleNames: ["Ghostty.app"] },
  {
    id: "terminal",
    displayName: "Terminal",
    kind: "terminal",
    bundleNames: ["Terminal.app"],
  },
]

/** Lists available worktree openers for the current desktop platform. */
export async function listWorktreeOpeners(input: {
  sessionId: string
}): Promise<WorktreeOpenersResponse> {
  void input
  const desktopPlatform = getDesktopPlatform()
  const openers = discoverWorktreeOpeners(desktopPlatform)
  const persistedOpenerId = await readLastUsedOpenerId()
  const primaryOpenerId = resolvePrimaryOpener(openers, persistedOpenerId)

  return {
    platform: desktopPlatform,
    openers: openers.map(
      ({ launchStrategy: _launchStrategy, discoveredPath: _discoveredPath, ...opener }) => opener,
    ),
    primaryOpenerId,
    persistedOpenerId,
  }
}

/** Opens one session worktree with the selected desktop opener and persists successful choices. */
export async function openWorktree(input: {
  sessionId: string
  openerId: WorktreeOpenerId
}): Promise<OpenWorktreeResponse> {
  try {
    const desktopPlatform = getDesktopPlatform()
    const opener = discoverWorktreeOpeners(desktopPlatform).find(
      (candidate) => candidate.id === input.openerId,
    )

    if (!opener) {
      return failure(input.openerId, "OPENER_NOT_FOUND", "The selected opener is not supported.")
    }

    if (!opener.available) {
      return failure(
        input.openerId,
        "OPENER_UNAVAILABLE",
        `${opener.displayName} is no longer available.`,
      )
    }

    const worktreePath = await resolveSessionWorktreePath(input.sessionId)

    if (!isDirectory(worktreePath)) {
      return failure(input.openerId, "WORKTREE_NOT_FOUND", "This worktree no longer exists.")
    }

    await launchWorktree(opener.launchStrategy, worktreePath)
    await writeLastUsedOpenerId(input.openerId)
    return { ok: true, openerId: input.openerId }
  } catch (error) {
    return failure(input.openerId, mapLaunchError(error), getLaunchErrorMessage(error))
  }
}

function getDesktopPlatform(): DesktopPlatform {
  switch (platform()) {
    case "darwin":
      return "macos"
    case "win32":
      return "windows"
    default:
      return "linux"
  }
}

function discoverWorktreeOpeners(desktopPlatform: DesktopPlatform) {
  switch (desktopPlatform) {
    case "macos":
      return sortOpeners([...discoverMacosApps(), createFinderOpener()])
    case "windows":
      return [createExplorerOpener()]
    case "linux":
      return [createLinuxFilesOpener()]
  }
}

function discoverMacosApps() {
  const applicationDirs = [
    "/Applications",
    join(homedir(), "Applications"),
    "/Applications/Utilities",
    "/System/Applications",
    "/System/Applications/Utilities",
  ]
  const openers: NativeWorktreeOpener[] = []

  for (const app of macosApps) {
    const appBundlePath = findFirstExistingBundle(applicationDirs, app.bundleNames)

    if (!appBundlePath) {
      continue
    }

    openers.push({
      id: app.id,
      displayName: app.displayName,
      kind: app.kind,
      platform: "macos",
      available: true,
      discoveredPath: appBundlePath,
      launchStrategy: { type: "macos-open-app", appBundlePath },
    })
  }

  return openers
}

function findFirstExistingBundle(applicationDirs: string[], bundleNames: string[]) {
  for (const applicationDir of applicationDirs) {
    for (const bundleName of bundleNames) {
      const bundlePath = join(applicationDir, bundleName)

      if (isDirectory(bundlePath)) {
        return bundlePath
      }
    }
  }

  return null
}

function createFinderOpener(): NativeWorktreeOpener {
  return {
    id: "finder",
    displayName: "Finder",
    kind: "file-manager",
    platform: "macos",
    available: true,
    launchStrategy: { type: "macos-finder" },
  }
}

function createExplorerOpener(): NativeWorktreeOpener {
  return {
    id: "explorer",
    displayName: "File Explorer",
    kind: "file-manager",
    platform: "windows",
    available: true,
    launchStrategy: { type: "windows-explorer" },
  }
}

function createLinuxFilesOpener(): NativeWorktreeOpener {
  return {
    id: "linux-files",
    displayName: "Files",
    kind: "file-manager",
    platform: "linux",
    available: true,
    launchStrategy: { type: "linux-open-location" },
  }
}

function sortOpeners(openers: NativeWorktreeOpener[]) {
  return [...openers].sort(
    (left, right) => preferredOrder.indexOf(left.id) - preferredOrder.indexOf(right.id),
  )
}

function resolvePrimaryOpener(
  openers: NativeWorktreeOpener[],
  persistedOpenerId: WorktreeOpenerId | null,
) {
  const available = new Set(openers.filter((opener) => opener.available).map((opener) => opener.id))

  if (persistedOpenerId && available.has(persistedOpenerId)) {
    return persistedOpenerId
  }

  for (const openerId of preferredOrder) {
    if (available.has(openerId)) {
      return openerId
    }
  }

  return null
}

async function resolveSessionWorktreePath(sessionId: string) {
  const worktreeResponse = (await daemonSend({
    name: "session.worktree.get",
    payload: { id: sessionId },
  })) as SessionWorktreeResponse

  if (worktreeResponse.worktree?.worktreeDir) {
    return worktreeResponse.worktree.worktreeDir
  }

  const sessionResponse = (await daemonSend({
    name: "session.get",
    payload: { id: sessionId },
  })) as SessionGetResponse

  return sessionResponse.session.cwd
}

async function launchWorktree(strategy: NativeLaunchStrategy, worktreePath: string) {
  switch (strategy.type) {
    case "macos-open-app":
      await spawnAndWait("/usr/bin/open", ["-a", strategy.appBundlePath, worktreePath])
      return
    case "macos-finder":
      await spawnAndWait("/usr/bin/open", [worktreePath])
      return
    case "windows-explorer":
      await spawnAndWait("explorer.exe", [worktreePath])
      return
    case "linux-open-location":
      await spawnAndWait("xdg-open", [worktreePath])
  }
}

async function spawnAndWait(command: string, args: string[]) {
  const process = Bun.spawn([command, ...args], {
    stdout: "ignore",
    stderr: "pipe",
  })
  const exitCode = await process.exited

  if (exitCode !== 0) {
    const stderr = await new Response(process.stderr).text().catch(() => "")
    throw new Error(stderr.trim() || `${command} exited with code ${exitCode}.`)
  }
}

function isDirectory(path: string) {
  try {
    return existsSync(path) && statSync(path).isDirectory()
  } catch {
    return false
  }
}

async function readLastUsedOpenerId() {
  const preferences = await readJsonFile(preferencesPath, WorktreeOpenerPreferencesFile)
  const value = preferences?.lastUsedOpenerId
  return isWorktreeOpenerId(value) ? value : null
}

async function writeLastUsedOpenerId(openerId: WorktreeOpenerId) {
  await writeJsonFile(preferencesPath, {
    version: preferencesFileVersion,
    lastUsedOpenerId: openerId,
  })
}

function isWorktreeOpenerId(value: unknown): value is WorktreeOpenerId {
  return typeof value === "string" && preferredOrder.includes(value as WorktreeOpenerId)
}

function failure(
  openerId: WorktreeOpenerId,
  errorCode: WorktreeOpenErrorCode,
  message: string,
): OpenWorktreeResponse {
  return {
    ok: false,
    openerId,
    errorCode,
    message,
  }
}

function mapLaunchError(error: unknown): WorktreeOpenErrorCode {
  if (error && typeof error === "object" && "name" in error && error.name === "PermissionDenied") {
    return "PERMISSION_DENIED"
  }

  return "LAUNCH_FAILED"
}

function getLaunchErrorMessage(error: unknown) {
  return error instanceof Error && error.message.trim()
    ? error.message
    : "The selected opener could not open this worktree."
}
