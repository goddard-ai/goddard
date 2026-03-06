import { runDaemonCli } from "./index.ts";

const exitCode = await runDaemonCli(process.argv.slice(2));
process.exit(exitCode);
