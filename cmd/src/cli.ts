#!/usr/bin/env -S pnpm tsx
import { runCli } from "./index.ts";

const exitCode = await runCli(process.argv.slice(2));
process.exit(exitCode);
