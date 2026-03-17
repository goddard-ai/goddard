import { command, option, optional, runSafely, string, subcommands } from "cmd-ts"
import { runDaemon } from "./daemon.ts"

const app = subcommands({
  name: "goddard-daemon",
  version: __VERSION__,
  cmds: {
    run: command({
      name: "run",
      args: {
        repo: option({
          type: string,
          long: "repo",
        }),
        projectDir: option({
          type: string,
          long: "project-dir",
          defaultValue: () => process.cwd(),
        }),
        baseUrl: option({
          type: string,
          long: "base-url",
          defaultValue: () => "http://goddardai.org/api",
        }),
        socketPath: option({
          type: optional(string),
          long: "socket-path",
        }),
        agentBinDir: option({
          type: optional(string),
          long: "agent-bin-dir",
        }),
      },
      handler: async (args) => {
        const exitCode = await runDaemon({
          repo: args.repo,
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

await runSafely(app, process.argv.slice(2))
