import { join } from "node:path"

import {
  artifactManifestPath,
  artifactPath,
  parseOptions,
  printResult,
  rootDir,
  run,
} from "./common.ts"

const options = parseOptions()
const scriptDir = join(rootDir, "scripts")
const targetArgs = ["--target", options.target]

await run(process.execPath, [join(scriptDir, "fetch.ts")])
await run(process.execPath, [join(scriptDir, "build.ts"), ...targetArgs])
await run(process.execPath, [join(scriptDir, "verify.ts"), ...targetArgs])

await printResult(
  {
    target: options.target,
    libraryPath: await artifactPath(options.target),
    manifestPath: await artifactManifestPath(options.target),
  },
  options.json,
)
