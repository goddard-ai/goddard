#!/usr/bin/env bun
/** Scaffolds internal full-stack feature packages from the repository root. */
import { spawnSync } from "node:child_process"
import { mkdir, mkdtemp, stat, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { basename, dirname, join, relative } from "node:path"
import { cancel, confirm, intro, isCancel, log, multiselect, outro, text } from "@clack/prompts"
import { command, flag, option, optional, run, string } from "cmd-ts"
import { dedent, getErrorMessage } from "radashi"

const FEATURE_LAYERS = ["daemon", "sdk", "app", "backend"] as const
const DEFAULT_LAYERS = ["daemon", "sdk", "app"] as const
const FEATURE_NAME_PATTERN = /^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/
const RESERVED_IDENTIFIERS = new Set([
  "class",
  "const",
  "default",
  "export",
  "function",
  "import",
  "let",
  "new",
  "package",
  "return",
  "switch",
  "var",
])

type FeatureLayer = (typeof FEATURE_LAYERS)[number]

/** File content and destination produced by the scaffold plan. */
export type ScaffoldFile = {
  path: string
  content: string
}

/** Fully resolved scaffold output ready to write or inspect. */
export type FeatureScaffoldPlan = {
  rootDir: string
  featureDir: string
  name: string
  packageName: string
  layers: FeatureLayer[]
  files: ScaffoldFile[]
}

/** Inputs accepted by the scaffold planner after prompts or flags are resolved. */
export type FeatureScaffoldOptions = {
  name: string
  rootDir?: string
  layers: readonly FeatureLayer[]
  includeSchema?: boolean
  includeDaemonIpc?: boolean
  includeStyledSystem?: boolean
}

type ParsedScaffoldArgs = {
  name?: string
  layers?: FeatureLayer[]
  includeSchema?: boolean
  includeDaemonIpc?: boolean
  includeStyledSystem?: boolean
  dryRun?: boolean
  rootDir?: string
  skipInstall?: boolean
}

type RawScaffoldArgs = {
  name?: string
  layers?: string
  includeSchema: boolean
  includeDaemonIpc: boolean
  skipDaemonIpc: boolean
  includeStyledSystem: boolean
  dryRun: boolean
  rootDir?: string
  skipInstall: boolean
}

function isFeatureLayer(value: string): value is FeatureLayer {
  return FEATURE_LAYERS.includes(value as FeatureLayer)
}

/** Normalizes human input into the internal kebab-case package segment. */
export function normalizeFeatureName(input: string) {
  return input
    .trim()
    .replace(/^@goddard-ai\//, "")
    .replace(/([a-z0-9])([A-Z])/g, "$1-$2")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
}

function validateFeatureName(input: string | undefined) {
  if (!input) {
    return "Enter a feature package name."
  }

  const name = normalizeFeatureName(input)

  if (!name) {
    return "Enter a feature package name."
  }

  if (!FEATURE_NAME_PATTERN.test(name)) {
    return "Use a kebab-case name that starts with a letter."
  }
}

function toTitle(name: string) {
  return name
    .split("-")
    .map((part) => part[0]!.toUpperCase() + part.slice(1))
    .join(" ")
}

function toIdentifier(name: string) {
  const identifier = name
    .split("-")
    .map((part, index) => (index === 0 ? part : part[0]!.toUpperCase() + part.slice(1)))
    .join("")
    .replace(/^[^A-Za-z_$]/, "feature$&")

  return RESERVED_IDENTIFIERS.has(identifier) ? `${identifier}Feature` : identifier
}

function hasLayer(layers: readonly FeatureLayer[], layer: FeatureLayer) {
  return layers.includes(layer)
}

function isDaemonIpcNeeded(options: FeatureScaffoldOptions) {
  return (
    options.includeDaemonIpc === true &&
    hasLayer(options.layers, "daemon") &&
    hasLayer(options.layers, "sdk")
  )
}

function isStyledSystemNeeded(options: FeatureScaffoldOptions) {
  return options.includeStyledSystem === true && hasLayer(options.layers, "app")
}

function formatJson(value: unknown) {
  return `${JSON.stringify(value, null, 2)}\n`
}

function formatTsList(values: string[]) {
  return values.map((value) => `"${value}"`).join(", ")
}

function sortObject<T>(value: Record<string, T>) {
  return Object.fromEntries(
    Object.entries(value).sort(([left], [right]) => left.localeCompare(right)),
  )
}

function createExportTarget(sourceFile: string) {
  const outputName = basename(sourceFile).replace(/\.(ts|tsx)$/, "")

  return {
    types: {
      source: `./src/${sourceFile}`,
      default: `./dist/${outputName}.d.mts`,
    },
    bun: `./src/${sourceFile}`,
    import: `./dist/${outputName}.mjs`,
  }
}

function createPackageJson(options: FeatureScaffoldOptions, packageName: string) {
  const dependencies: Record<string, string> = {}
  const exports: Record<string, ReturnType<typeof createExportTarget>> = {}

  if (hasLayer(options.layers, "app")) {
    dependencies["@goddard-ai/app-plugin"] = "workspace:*"
    exports["./app"] = createExportTarget("app.tsx")
  }

  if (hasLayer(options.layers, "backend")) {
    exports["./backend"] = createExportTarget("backend.ts")
  }

  if (hasLayer(options.layers, "daemon")) {
    dependencies["@goddard-ai/daemon-plugin"] = "workspace:*"
    exports["./daemon"] = createExportTarget("daemon.ts")
  }

  if (isDaemonIpcNeeded(options)) {
    dependencies["@goddard-ai/ipc"] = "workspace:*"
    exports["./daemon-ipc"] = createExportTarget("daemon-ipc.ts")
  }

  if (hasLayer(options.layers, "sdk")) {
    dependencies["@goddard-ai/sdk-plugin"] = "workspace:*"
    exports["./sdk"] = createExportTarget("sdk.ts")
  }

  if (options.includeSchema === true) {
    dependencies.zod = "catalog:"
    exports["./schema"] = createExportTarget("schema.ts")
  }

  if (isStyledSystemNeeded(options)) {
    dependencies["@goddard-ai/styled-system"] = "workspace:*"
  }

  return {
    name: packageName,
    version: "0.1.0",
    private: true,
    license: "MIT",
    type: "module",
    exports: sortObject(exports),
    scripts: {
      build: "tsdown --unused",
      lint: "oxlint",
      fmt: "prettier -w .",
      typecheck: "tsgo --noEmit && tsgo -p test --noEmit",
      test: "bun test --dots",
    },
    dependencies: sortObject(dependencies),
  }
}

function createTsconfig(options: FeatureScaffoldOptions) {
  return {
    extends: "../../tsconfig.base.json",
    compilerOptions: hasLayer(options.layers, "app")
      ? {
          jsx: "react-jsx",
        }
      : undefined,
    include: hasLayer(options.layers, "app") ? ["src/**/*.ts", "src/**/*.tsx"] : ["src/**/*.ts"],
  }
}

function createTestTsconfig(options: FeatureScaffoldOptions) {
  return {
    extends: "../tsconfig.json",
    compilerOptions: {
      types: ["bun"],
      rootDir: "..",
      ...(hasLayer(options.layers, "app") ? { jsx: "react-jsx" } : {}),
    },
    include: ["."],
  }
}

function createTsdownConfig(options: FeatureScaffoldOptions) {
  const entries = [
    hasLayer(options.layers, "app") ? "./src/app.tsx" : undefined,
    hasLayer(options.layers, "backend") ? "./src/backend.ts" : undefined,
    hasLayer(options.layers, "daemon") ? "./src/daemon.ts" : undefined,
    isDaemonIpcNeeded(options) ? "./src/daemon-ipc.ts" : undefined,
    hasLayer(options.layers, "sdk") ? "./src/sdk.ts" : undefined,
    options.includeSchema === true ? "./src/schema.ts" : undefined,
  ].filter((entry): entry is string => Boolean(entry))

  return `${dedent`
    import { defineConfig } from "tsdown"

    const isDebug = process.env.DEBUG === "true"

    export default defineConfig({
      entry: [${formatTsList(entries)}],
      format: "esm",
      target: "node18",
      clean: true,
      outDir: "dist",
      sourcemap: isDebug,
      dts: {
        tsgo: true,
      },
    })
  `}\n`
}

function createSdkEntrypoint(name: string) {
  const identifier = toIdentifier(name)

  return `${dedent`
    import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

    export const ${identifier}SdkPlugin = defineSdkPlugin({
      name: "${name}",
      namespace: "${identifier}",
      create() {
        return {}
      },
    })
  `}\n`
}

function createDaemonIpcEntrypoint(name: string) {
  const identifier = toIdentifier(name)

  return `${dedent`
    import { defineIpcSchema } from "@goddard-ai/ipc"

    export const ${identifier}IpcSchema = defineIpcSchema({
      requests: {},
      streams: {},
    })
  `}\n`
}

function createDaemonEntrypoint(options: FeatureScaffoldOptions, name: string) {
  const identifier = toIdentifier(name)
  const ipcImport = isDaemonIpcNeeded(options)
    ? `import { ${identifier}IpcSchema } from "./daemon-ipc.ts"\n`
    : ""
  const ipcProperty = isDaemonIpcNeeded(options) ? `,\n  ipc: ${identifier}IpcSchema` : ""

  return `${dedent`
    import { defineDaemonPlugin } from "@goddard-ai/daemon-plugin"
    ${ipcImport}
    export const ${identifier}DaemonPlugin = defineDaemonPlugin({
      name: "${name}"${ipcProperty},
    })
  `}\n`
}

function createAppEntrypoint(options: FeatureScaffoldOptions, name: string) {
  const identifier = toIdentifier(name)
  const styleImport = isStyledSystemNeeded(options)
    ? `import { ${identifier}RootClass } from "./app.style.ts"\n`
    : ""
  const sdkRequirement = hasLayer(options.layers, "sdk")
    ? `,\n  sdk: {\n    namespaces: ["${identifier}"],\n  }`
    : ""
  const styles = isStyledSystemNeeded(options)
    ? `,\n  styles: {\n    rootClass: ${identifier}RootClass,\n  }`
    : ""

  return `${dedent`
    import { defineAppPlugin } from "@goddard-ai/app-plugin"
    ${styleImport}
    export const ${identifier}AppPlugin = defineAppPlugin({
      name: "${name}",
      routes: [],
      commands: []${sdkRequirement}${styles},
    })
  `}\n`
}

function createAppStyle(name: string) {
  const identifier = toIdentifier(name)

  return `${dedent`
    import { css } from "@goddard-ai/styled-system/css"

    export const ${identifier}RootClass = css({
      display: "contents",
    })
  `}\n`
}

function createBackendEntrypoint(name: string) {
  const identifier = toIdentifier(name)

  return `${dedent`
    export const ${identifier}BackendEntrypoint = {
      name: "${name}",
    }
  `}\n`
}

function createSchemaEntrypoint(name: string) {
  const identifier = toIdentifier(name)
  const typeName = `${toTitle(name).replace(/\s+/g, "")}Id`

  return `${dedent`
    import { z } from "zod"

    export const ${identifier}IdSchema = z.string().min(1)

    export type ${typeName} = z.infer<typeof ${identifier}IdSchema>
  `}\n`
}

function createEntrypointTest(options: FeatureScaffoldOptions, name: string) {
  const identifier = toIdentifier(name)
  const imports = [
    hasLayer(options.layers, "app")
      ? `import { ${identifier}AppPlugin } from "../src/app.tsx"`
      : undefined,
    hasLayer(options.layers, "backend")
      ? `import { ${identifier}BackendEntrypoint } from "../src/backend.ts"`
      : undefined,
    hasLayer(options.layers, "daemon")
      ? `import { ${identifier}DaemonPlugin } from "../src/daemon.ts"`
      : undefined,
    isDaemonIpcNeeded(options)
      ? `import { ${identifier}IpcSchema } from "../src/daemon-ipc.ts"`
      : undefined,
    hasLayer(options.layers, "sdk")
      ? `import { ${identifier}SdkPlugin } from "../src/sdk.ts"`
      : undefined,
    options.includeSchema === true
      ? `import { ${identifier}IdSchema } from "../src/schema.ts"`
      : undefined,
  ].filter((entry): entry is string => Boolean(entry))

  const assertions = [
    hasLayer(options.layers, "app")
      ? `    expect(${identifier}AppPlugin.name).toBe("${name}")`
      : undefined,
    hasLayer(options.layers, "backend")
      ? `    expect(${identifier}BackendEntrypoint.name).toBe("${name}")`
      : undefined,
    hasLayer(options.layers, "daemon")
      ? `    expect(${identifier}DaemonPlugin.name).toBe("${name}")`
      : undefined,
    isDaemonIpcNeeded(options)
      ? `    expect(${identifier}IpcSchema).toEqual({ requests: {}, streams: {} })`
      : undefined,
    hasLayer(options.layers, "sdk")
      ? `    expect(${identifier}SdkPlugin.namespace).toBe("${identifier}")`
      : undefined,
    options.includeSchema === true
      ? `    expect(${identifier}IdSchema.parse("${name}")).toBe("${name}")`
      : undefined,
  ].filter((entry): entry is string => Boolean(entry))

  return `${dedent`
    import { describe, expect, test } from "bun:test"
    ${imports.join("\n")}

    describe("${name} feature package", () => {
      test("exports selected feature entrypoints", () => {
    ${assertions.join("\n")}
      })
    })
  `}\n`
}

function addFile(files: ScaffoldFile[], featureDir: string, path: string, content: string) {
  files.push({
    path: join(featureDir, path),
    content,
  })
}

/** Creates the full file plan for a feature package without touching the filesystem. */
export function createFeatureScaffoldPlan(options: FeatureScaffoldOptions) {
  const name = normalizeFeatureName(options.name)
  const validationError = validateFeatureName(name)

  if (validationError) {
    throw new Error(validationError)
  }

  if (options.layers.length === 0) {
    throw new Error("Select at least one feature layer.")
  }

  const rootDir = options.rootDir ?? process.cwd()
  const packageName = `@goddard-ai/${name}`
  const featureDir = join(rootDir, "features", name)
  const files: ScaffoldFile[] = []

  addFile(files, featureDir, "package.json", formatJson(createPackageJson(options, packageName)))
  addFile(files, featureDir, "tsconfig.json", formatJson(createTsconfig(options)))
  addFile(files, featureDir, "test/tsconfig.json", formatJson(createTestTsconfig(options)))
  addFile(files, featureDir, "tsdown.config.ts", createTsdownConfig(options))
  addFile(files, featureDir, "test/feature.test.ts", createEntrypointTest(options, name))

  if (hasLayer(options.layers, "app")) {
    addFile(files, featureDir, "src/app.tsx", createAppEntrypoint(options, name))
  }

  if (isStyledSystemNeeded(options)) {
    addFile(files, featureDir, "src/app.style.ts", createAppStyle(name))
  }

  if (hasLayer(options.layers, "backend")) {
    addFile(files, featureDir, "src/backend.ts", createBackendEntrypoint(name))
  }

  if (hasLayer(options.layers, "daemon")) {
    addFile(files, featureDir, "src/daemon.ts", createDaemonEntrypoint(options, name))
  }

  if (isDaemonIpcNeeded(options)) {
    addFile(files, featureDir, "src/daemon-ipc.ts", createDaemonIpcEntrypoint(name))
  }

  if (hasLayer(options.layers, "sdk")) {
    addFile(files, featureDir, "src/sdk.ts", createSdkEntrypoint(name))
  }

  if (options.includeSchema === true) {
    addFile(files, featureDir, "src/schema.ts", createSchemaEntrypoint(name))
  }

  return {
    rootDir,
    featureDir,
    name,
    packageName,
    layers: [...options.layers],
    files,
  } satisfies FeatureScaffoldPlan
}

async function assertFeatureDirAvailable(featureDir: string) {
  try {
    await stat(featureDir)
  } catch (error) {
    if (error instanceof Error && "code" in error && error.code === "ENOENT") {
      return
    }

    throw error
  }

  throw new Error(`Feature package already exists: ${featureDir}`)
}

/** Writes a scaffold plan after confirming that the feature directory is new. */
export async function writeFeatureScaffoldPlan(plan: FeatureScaffoldPlan) {
  await assertFeatureDirAvailable(plan.featureDir)

  for (const file of plan.files) {
    await mkdir(dirname(file.path), { recursive: true })
    await writeFile(file.path, file.content)
  }
}

function parseLayerList(value: string) {
  const values = value
    .split(",")
    .map((layer) => layer.trim())
    .filter(Boolean)
  const layers: FeatureLayer[] = []

  for (const layer of values) {
    if (!isFeatureLayer(layer)) {
      throw new Error(`Unknown feature layer: ${layer}`)
    }

    layers.push(layer)
  }

  return layers
}

function resolveRawScaffoldArgs(raw: RawScaffoldArgs) {
  const parsed: ParsedScaffoldArgs = {}

  if (raw.name !== undefined) {
    parsed.name = raw.name
  }

  if (raw.rootDir !== undefined) {
    parsed.rootDir = raw.rootDir
  }

  if (raw.layers !== undefined) {
    parsed.layers = parseLayerList(raw.layers)
  }

  if (raw.includeSchema) {
    parsed.includeSchema = true
  }

  if (raw.includeStyledSystem) {
    parsed.includeStyledSystem = true
  }

  if (raw.skipDaemonIpc) {
    parsed.includeDaemonIpc = false
  } else if (raw.includeDaemonIpc) {
    parsed.includeDaemonIpc = true
  }

  if (raw.dryRun) {
    parsed.dryRun = true
  }

  if (raw.skipInstall) {
    parsed.skipInstall = true
  }

  return parsed
}

const scaffoldCommandArgs = {
  name: option({
    type: optional(string),
    long: "name",
    description: "Feature package name, for example inbox",
  }),
  layers: option({
    type: optional(string),
    long: "layers",
    description: "Comma-separated layers: daemon,sdk,app,backend",
  }),
  includeSchema: flag({
    long: "schema",
    description: "Generate src/schema.ts with a starter Zod schema",
    defaultValue: () => false,
  }),
  includeDaemonIpc: flag({
    long: "daemon-ipc",
    description: "Generate src/daemon-ipc.ts when daemon and sdk are selected",
    defaultValue: () => false,
  }),
  skipDaemonIpc: flag({
    long: "no-daemon-ipc",
    description: "Skip src/daemon-ipc.ts",
    defaultValue: () => false,
  }),
  includeStyledSystem: flag({
    long: "styled-system",
    description: "Generate app.style.ts and depend on @goddard-ai/styled-system",
    defaultValue: () => false,
  }),
  dryRun: flag({
    long: "dry-run",
    description: "Print planned files without writing",
    defaultValue: () => false,
  }),
  skipInstall: flag({
    long: "skip-install",
    description: "Do not run bun install after writing files",
    defaultValue: () => false,
  }),
  rootDir: option({
    type: optional(string),
    long: "root",
    description: "Repository root, defaults to the current directory",
  }),
}

const parseCommand = command({
  name: "scaffold:feature",
  description: "Create an internal Goddard feature package scaffold",
  args: scaffoldCommandArgs,
  handler: resolveRawScaffoldArgs,
})

/** Parses noninteractive flags used by agents and tests. */
export function parseScaffoldArgs(args: string[]) {
  return run(parseCommand, args)
}

function cancelPrompt(message: string) {
  cancel(message)
  process.exit(1)
}

function getPromptValue<T>(value: T | symbol, message: string) {
  if (isCancel(value)) {
    cancelPrompt(message)
  }

  return value as T
}

async function promptForOptions(parsed: ParsedScaffoldArgs) {
  intro("Create feature package")
  const usePromptDefaults = parsed.name !== undefined && parsed.layers !== undefined

  const name =
    parsed.name ??
    (await text({
      message: "Feature package name",
      placeholder: "inbox",
      validate: validateFeatureName,
    }))

  const resolvedName = getPromptValue<string>(name, "Feature scaffold cancelled.")

  const layers =
    parsed.layers ??
    (await multiselect<FeatureLayer>({
      message: "Which layers does this feature need?",
      initialValues: [...DEFAULT_LAYERS],
      required: true,
      options: [
        { value: "daemon", label: "daemon", hint: "local runtime, IPC handlers, background work" },
        { value: "sdk", label: "sdk", hint: "public SDK namespace bundled by core/sdk" },
        { value: "app", label: "app", hint: "UI, commands, navigation metadata" },
        { value: "backend", label: "backend", hint: "worker-hosted authority or persistence" },
      ],
    }))

  const resolvedLayers = getPromptValue<FeatureLayer[]>(layers, "Feature scaffold cancelled.")

  const includeSchema =
    parsed.includeSchema ??
    (usePromptDefaults
      ? false
      : await confirm({
          message: "Generate shared Zod schema entrypoint?",
          initialValue: false,
        }))

  const resolvedIncludeSchema = getPromptValue<boolean>(
    includeSchema,
    "Feature scaffold cancelled.",
  )

  const includeStyledSystem =
    parsed.includeStyledSystem ??
    (usePromptDefaults
      ? false
      : hasLayer(resolvedLayers, "app")
        ? await confirm({
            message: "Generate app style entrypoint with @goddard-ai/styled-system?",
            initialValue: false,
          })
        : false)

  const resolvedIncludeStyledSystem = getPromptValue<boolean>(
    includeStyledSystem,
    "Feature scaffold cancelled.",
  )

  const includeDaemonIpc =
    parsed.includeDaemonIpc ??
    (hasLayer(resolvedLayers, "daemon") && hasLayer(resolvedLayers, "sdk")
      ? usePromptDefaults
        ? true
        : await confirm({
            message: "Generate shared daemon IPC contract?",
            initialValue: true,
          })
      : false)

  const resolvedIncludeDaemonIpc = getPromptValue<boolean>(
    includeDaemonIpc,
    "Feature scaffold cancelled.",
  )

  return {
    name: resolvedName,
    rootDir: parsed.rootDir,
    layers: resolvedLayers,
    includeSchema: resolvedIncludeSchema,
    includeStyledSystem: resolvedIncludeStyledSystem,
    includeDaemonIpc: resolvedIncludeDaemonIpc,
  } satisfies FeatureScaffoldOptions
}

function installWorkspaceDependencies(rootDir: string) {
  const result = spawnSync(process.execPath, ["install"], {
    cwd: rootDir,
    stdio: "inherit",
  })

  if (result.status !== 0) {
    throw new Error("bun install failed after feature scaffold.")
  }
}

async function main(args = process.argv.slice(2)) {
  const parsed = await parseScaffoldArgs(args)
  const options = await promptForOptions(parsed)
  const plan = createFeatureScaffoldPlan(options)

  if (parsed.dryRun) {
    log.info(
      `Would create ${plan.packageName} with ${plan.layers.join(", ")} layer(s):\n${plan.files
        .map((file) => `- ${relative(plan.rootDir, file.path)}`)
        .join("\n")}`,
    )
    outro("Dry run complete.")
    return
  }

  await writeFeatureScaffoldPlan(plan)

  if (!parsed.skipInstall) {
    installWorkspaceDependencies(plan.rootDir)
  }

  outro(`Created ${relative(plan.rootDir, plan.featureDir)}.`)
}

if (import.meta.main) {
  main().catch((error) => {
    process.stderr.write(`${getErrorMessage(error)}\n`)
    process.exit(1)
  })
}

export async function createTempScaffoldRoot() {
  return mkdtemp(join(tmpdir(), "goddard-feature-scaffold-"))
}
