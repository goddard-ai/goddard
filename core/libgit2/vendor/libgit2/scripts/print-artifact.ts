import {
  artifactManifestPath,
  artifactPath,
  assertFile,
  parseOptions,
  printResult,
  resolveTarget,
} from "./common.ts"

const options = parseOptions()
const targetConfig = await resolveTarget(options.target)
const libraryPath = await artifactPath(options.target)
const manifestPath = await artifactManifestPath(options.target)

await assertFile(libraryPath)
await assertFile(manifestPath)

await printResult(
  {
    target: options.target,
    bunTarget: targetConfig.bunTarget,
    libraryPath,
    manifestPath,
  },
  options.json,
)
