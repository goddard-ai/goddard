#!/usr/bin/env node
import { runReviewSync } from "./index.ts"
import { writeResult } from "./runtime.ts"

const result = await runReviewSync(process.argv.slice(2))

writeResult(result)
process.exitCode = result.exitCode
