#!/usr/bin/env node
import { cancel, intro, isCancel, multiselect, outro } from "@clack/prompts"
import { command, option, optional, runSafely, string, subcommands } from "cmd-ts"
import {
  discoverWorkforceInitCandidates,
  initializeWorkforcePackages,
  resolveRepositoryRoot,
  watchWorkforce,
  type DiscoveredWorkforcePackage,
  type WorkforceRuntimeEvent,
} from "@goddard-ai/sdk/node"

function formatPackageLabel(pkg: DiscoveredWorkforcePackage): string {
  return pkg.relativeDir === "."
    ? `${pkg.name} (repository root)`
    : `${pkg.name} (${pkg.relativeDir})`
}

export async function promptForWorkforcePackages(
  candidates: DiscoveredWorkforcePackage[],
): Promise<string[] | null> {
  const selected = await multiselect({
    message: "Select packages to initialize for workforce features.",
    options: candidates.map((candidate) => ({
      value: candidate.rootDir,
      label: formatPackageLabel(candidate),
      hint: candidate.relativeDir === "." ? "root agent" : candidate.relativeDir,
    })),
  })

  if (isCancel(selected)) {
    cancel("Workforce initialization cancelled.")
    return null
  }

  return selected
}

export async function runInitCommand(args: { root: string }): Promise<void> {
  const repositoryRoot = await resolveRepositoryRoot(args.root)
  const candidates = await discoverWorkforceInitCandidates(repositoryRoot)

  if (candidates.length === 0) {
    console.log(`No workforce candidates found under ${repositoryRoot}.`)
    return
  }

  intro(`Initializing workforce packages in ${repositoryRoot}`)
  const selectedPackageDirs = await promptForWorkforcePackages(candidates)
  if (selectedPackageDirs === null) {
    return
  }

  if (selectedPackageDirs.length === 0) {
    outro("No packages selected.")
    return
  }

  const initializedPackages = await initializeWorkforcePackages(selectedPackageDirs)
  const initializedCount = initializedPackages.filter((pkg) => pkg.createdPaths.length > 0).length

  outro(
    `Initialized ${initializedPackages.length} package(s); ${initializedCount} created new workforce files.`,
  )
}

export function logWatchEvent(event: WorkforceRuntimeEvent): void {
  switch (event.type) {
    case "package-discovered":
      console.log(`Discovered workforce package: ${formatPackageLabel(event.package)}`)
      return
    case "session-started":
      console.log(
        `Started workforce session for ${formatPackageLabel(event.package)} (${event.sessionId})`,
      )
      return
    case "batch-prompted":
      console.log(
        `Prompted ${formatPackageLabel(event.package)} with ${event.batchCount} batch(es) and ${event.lineCount} line(s).`,
      )
      return
    case "batch-queued":
      console.log(
        `Queued follow-up work for ${formatPackageLabel(event.package)}: ${event.batchCount} batch(es), ${event.lineCount} line(s).`,
      )
      return
    case "runtime-error":
      console.error(
        `Workforce runtime error for ${formatPackageLabel(event.package)}: ${event.error.message}`,
      )
  }
}

export async function waitForShutdownSignal(): Promise<void> {
  await new Promise<void>((resolve) => {
    let resolved = false

    const handleSignal = () => {
      if (resolved) {
        return
      }

      resolved = true
      process.off("SIGINT", handleSignal)
      process.off("SIGTERM", handleSignal)
      resolve()
    }

    process.on("SIGINT", handleSignal)
    process.on("SIGTERM", handleSignal)
  })
}

export async function runWatchCommand(
  args: { root: string; daemonUrl?: string },
  deps: {
    waitForShutdownSignal?: () => Promise<void>
    logWatchEvent?: (event: WorkforceRuntimeEvent) => void
  } = {},
): Promise<void> {
  const repositoryRoot = await resolveRepositoryRoot(args.root)
  const supervisor = await watchWorkforce({
    rootDir: repositoryRoot,
    daemon: args.daemonUrl ? { daemonUrl: args.daemonUrl } : undefined,
    onEvent: deps.logWatchEvent ?? logWatchEvent,
  })

  console.log(`Watching workforce packages under ${repositoryRoot}. Press Ctrl+C to stop.`)

  try {
    await (deps.waitForShutdownSignal ?? waitForShutdownSignal)()
  } finally {
    await supervisor.stop()
  }
}

export async function main(argv: string[]) {
  const app = subcommands({
    name: "goddard-workforce",
    cmds: {
      init: command({
        name: "init",
        args: {
          root: option({
            type: string,
            long: "root",
            defaultValue: () => process.cwd(),
          }),
        },
        handler: runInitCommand,
      }),
      watch: command({
        name: "watch",
        args: {
          root: option({
            type: string,
            long: "root",
            defaultValue: () => process.cwd(),
          }),
          daemonUrl: option({
            type: optional(string),
            long: "daemon-url",
          }),
        },
        handler: runWatchCommand,
      }),
    },
  })

  await runSafely(app, argv)
}

if (import.meta.main) {
  await main(process.argv.slice(2))
}
