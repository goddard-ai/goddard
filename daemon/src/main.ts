import { command, option, runSafely, string, subcommands } from "cmd-ts"
import { runDaemon } from "./daemon.ts"

const runCmd = command({
  name: "run",
  args: {
    repo: option({ type: string, long: "repo" }),
    projectDir: option({ type: string, long: "project-dir", defaultValue: () => process.cwd() }),
    baseUrl: option({ type: string, long: "base-url", defaultValue: () => "" }),
    socketPath: option({ type: string, long: "socket-path", defaultValue: () => "" }),
    agentBinDir: option({ type: string, long: "agent-bin-dir", defaultValue: () => "" }),
  },
  handler: async (args) => {
    return runDaemon({
      repo: args.repo,
      projectDir: args.projectDir,
      baseUrl: args.baseUrl,
      socketPath: args.socketPath || undefined,
      agentBinDir: args.agentBinDir || undefined,
    })
  },
})

const app = subcommands({
  name: "goddard-daemon",
  cmds: { run: runCmd },
})

const result = await runSafely(app, process.argv.slice(2))
let exitCode = 0

if (result._tag === "error") {
  process.stderr.write(`${result.error.config.message}\n`)
  exitCode = result.error.config.exitCode
} else if (typeof result.value === "number") {
  exitCode = result.value
} else if (result.value && typeof (result.value as any).value === "number") {
  exitCode = (result.value as any).value
}

process.exit(exitCode)
