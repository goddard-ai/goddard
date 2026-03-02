#!/usr/bin/env -S pnpm tsx
import { runCli } from "../src/index.ts";

const exitCode = await runCli(process.argv.slice(2));
process.exit(exitCode);
