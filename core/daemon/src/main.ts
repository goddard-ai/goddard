import {
  command,
  flag,
  oneOf,
  option,
  optional,
  restPositionals,
  run,
  string,
  subcommands,
} from "cmd-ts"
import { runDaemon } from "./daemon.ts"

declare const __VERSION__: string

const daemonRunFeatures = ["ipc", "stream"] as const

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

function resolveLogMode(options: { json: boolean; verbose: boolean }) {
  if (options.verbose) {
    return "verbose" as const
  }

  if (options.json) {
    return "json" as const
  }

  return "pretty" as const
}

/** Runs the daemon CLI with the provided process arguments. */
export async function main(argv = process.argv.slice(2)) {
  const app = subcommands({
    name: "goddard-daemon",
    version: __VERSION__,
    description: "Goddard background daemon for IPC, automation, and unified event handling",
    cmds: {
      run: command({
        name: "run",
        description: "Start the daemon runtime and background services",
        args: {
          baseUrl: option({
            type: string,
            long: "base-url",
            defaultValue: () => "https://goddardai.org/api",
            description: "Base URL for the Goddard API",
          }),
          socketPath: option({
            type: optional(string),
            long: "socket-path",
            description: "Unix socket path for daemon IPC control",
          }),
          agentBinDir: option({
            type: optional(string),
            long: "agent-bin-dir",
            description: "Directory containing agent executables used by daemon-managed sessions",
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
          const featureFlags = resolveRunFeatureFlags(args.features)
          const exitCode = await runDaemon({
            baseUrl: args.baseUrl,
            socketPath: args.socketPath,
            agentBinDir: args.agentBinDir,
            enableIpc: featureFlags.enableIpc,
            enableStream: featureFlags.enableStream,
            logMode: resolveLogMode(args),
          })
          process.exit(exitCode)
        },
      }),
    },
  })

  await run(app, argv)
}

if (import.meta.main) {
  await main()
}
