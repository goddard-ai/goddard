import { command, option, optional, run, string, subcommands } from "cmd-ts"
import { runDaemon } from "./daemon.ts"

declare const __VERSION__: string

const app = subcommands({
  name: "goddard-daemon",
  version: __VERSION__,
  description: "Goddard background daemon for IPC, automation, and unified event handling",
  cmds: {
    run: command({
      name: "run",
      description: "Start the daemon runtime and background services",
      args: {
        projectDir: option({
          type: string,
          long: "project-dir",
          defaultValue: () => process.cwd(),
          description: "Local directory of the project",
        }),
        baseUrl: option({
          type: string,
          long: "base-url",
          defaultValue: () => "https://goddardai.org/api",
          description: "Base URL for the Goddard API",
        }),
        socketPath: option({
          type: optional(string),
          long: "socket-path",
          description: "Path to the Unix socket for IPC",
        }),
        agentBinDir: option({
          type: optional(string),
          long: "agent-bin-dir",
          description: "Directory containing agent binaries",
        }),
      },
      handler: async (args) => {
        const exitCode = await runDaemon({
          projectDir: args.projectDir,
          baseUrl: args.baseUrl,
          socketPath: args.socketPath,
          agentBinDir: args.agentBinDir,
        })
        process.exit(exitCode)
      },
    }),
  },
})

await run(app, process.argv.slice(2))
