import { runSessionCli } from "./cli.ts"

const exitCode = await runSessionCli(process.argv.slice(2))
process.exit(exitCode)
