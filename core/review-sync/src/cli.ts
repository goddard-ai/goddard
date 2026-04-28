#!/usr/bin/env node
import { runReviewSync } from "./index.ts"

const result = await runReviewSync(process.argv.slice(2), {
  cwd: process.cwd(),
  env: process.env,
  stdout: process.stdout,
  stderr: process.stderr,
})

process.exitCode = result.exitCode
