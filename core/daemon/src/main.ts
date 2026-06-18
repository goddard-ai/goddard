#!/usr/bin/env bun
import {
  command,
  flag,
  oneOf,
  option,
  optional,
  positional,
  restPositionals,
  run,
  string,
  subcommands,
} from "cmd-ts"

declare const __VERSION__: string

const daemonRunFeatures = ["ipc", "stream"] as const
const daemonDataProfiles = ["development", "mock", "production"] as const
const daemonSeedProfiles = ["mock"] as const

/** Falls back to a placeholder version when the build-time constant is unavailable. */
function getPackageVersion(): string {
  try {
    return __VERSION__
  } catch {
    return "0.0.0"
  }
}

/** Maps positional feature arguments into the daemon runtime feature toggles. */
function resolveRunFeatureFlags(features: readonly (typeof daemonRunFeatures)[number][]) {
  if (features.length === 0) {
    return {
      enableIpc: true,
      enableStream: true,
    }
  }

  const enabled = new Set(features)
  return {
    enableIpc: enabled.has("ipc"),
    enableStream: enabled.has("stream"),
  }
}

/** Chooses the structured logging renderer requested by the CLI flags. */
function resolveLogMode(options: { json: boolean; verbose: boolean }) {
  if (options.verbose) {
    return "verbose" as const
  }

  if (options.json) {
    return "json" as const
  }

  return "compact" as const
}

/** Persists the selected daemon data profile before runtime modules initialize the store. */
function applyDataProfile(value?: (typeof daemonDataProfiles)[number]) {
  if (!value) {
    return
  }

  process.env.GODDARD_DATA_PROFILE = value
}

/** Persists packaged review-sync native library location before runtime modules initialize. */
function applyReviewSyncLibgit2Path(value?: string) {
  if (!value) {
    return
  }

  process.env.REVIEW_SYNC_LIBGIT2_PATH = value
}

function resolveCliPort(value?: string) {
  if (!value) {
    return undefined
  }

  const port = Number(value)
  if (!Number.isInteger(port) || port < 1 || port > 65535) {
    throw new Error("--port must be an integer TCP port between 1 and 65535")
  }

  return port
}

/** Runs the daemon CLI with the provided process arguments. */
export async function main(argv = process.argv.slice(2)) {
  const app = subcommands({
    name: "goddard-daemon",
    version: getPackageVersion(),
    description: "Goddard background daemon for IPC, automation, and unified event handling",
    cmds: {
      run: command({
        name: "run",
        description: "Start the daemon runtime and background services",
        args: {
          baseUrl: option({
            type: optional(string),
            long: "base-url",
            description: "Base URL for the Goddard API",
          }),
          port: option({
            type: optional(string),
            long: "port",
            description: "TCP port for daemon IPC control",
          }),
          agentBinDir: option({
            type: optional(string),
            long: "agent-bin-dir",
            description: "Directory containing agent executables used by daemon-managed sessions",
          }),
          reviewSyncLibgit2Path: option({
            type: optional(string),
            long: "review-sync-libgit2-path",
            description: "Private packaged libgit2 path used by review-sync",
          }),
          dataProfile: option({
            type: optional(oneOf(daemonDataProfiles)),
            long: "data-profile",
            description:
              "Persistence profile for daemon-managed data. Use development to isolate local dev data from the default profile.",
          }),
          json: flag({
            long: "json",
            description: "Render raw structured daemon logs as JSON lines",
          }),
          verbose: flag({
            long: "verbose",
            description: "Render full daemon log payloads in an expanded human-readable format",
          }),
          features: restPositionals({
            type: oneOf(daemonRunFeatures),
            displayName: "feature",
            description:
              "Optional runtime features to enable. Supported values: ipc and stream; omit all features to enable everything",
          }),
        },
        handler: async (args) => {
          applyDataProfile(args.dataProfile)
          applyReviewSyncLibgit2Path(args.reviewSyncLibgit2Path)

          // Load the runtime only when executing `run` so `--help` stays side-effect free.
          const { runDaemon } = await import("./daemon.ts")
          const featureFlags = resolveRunFeatureFlags(args.features)
          const exitCode = await runDaemon({
            baseUrl: args.baseUrl,
            port: resolveCliPort(args.port),
            agentBinDir: args.agentBinDir,
            reviewSyncLibgit2Path: args.reviewSyncLibgit2Path,
            enableIpc: featureFlags.enableIpc,
            enableStream: featureFlags.enableStream,
            logMode: resolveLogMode(args),
          })
          process.exit(exitCode)
        },
      }),
      seed: command({
        name: "seed",
        description: "Seed isolated daemon data profiles with deterministic local data",
        args: {
          profile: positional({
            type: oneOf(daemonSeedProfiles),
            displayName: "profile",
            description: "Data profile to seed. Supported value: mock",
          }),
          reset: flag({
            long: "reset",
            description: "Delete existing mock database artifacts before seeding",
          }),
        },
        handler: async ({ profile, reset }) => {
          if (profile === "mock") {
            const { seedMockData } = await import("./seed/mock.ts")
            const result = await seedMockData({ reset })
            console.log(`Seeded mock daemon data at ${result.databasePath}`)
          }
        },
      }),
      "terminal-check": command({
        name: "terminal-check",
        description: "Validate daemon PTY spawn, write, resize, and close behavior",
        args: {
          json: flag({
            long: "json",
            description: "Render the terminal runtime check result as JSON",
          }),
        },
        handler: async (args) => {
          const { runTerminalRuntimeCheck } = await import("@goddard-ai/terminal/daemon")
          const result = await runTerminalRuntimeCheck()
          if (args.json) {
            console.log(JSON.stringify(result))
          } else {
            console.log(result.ok ? "daemon terminal runtime ok" : "daemon terminal runtime failed")
          }
          process.exit(result.ok ? 0 : 1)
        },
      }),
    },
  })

  await run(app, argv)
}

if (import.meta.main) {
  await main().catch(async (error) => {
    try {
      const { createLogger, createLogStore, toErrorProperties } = await import("@goddard-ai/logs")
      const store = createLogStore()

      try {
        createLogger({ scope: "daemon", store, pid: process.pid }).error(
          "daemon.cli_failed",
          toErrorProperties(error),
        )
      } finally {
        store.close()
      }
    } catch {
      // Preserve the original CLI failure when durable logging is unavailable.
    }

    console.error(error)
    process.exit(1)
  })
}
