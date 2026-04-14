/** Daemon-owned text-model loading that prefers bundled provider packages and can install others on demand. */
import { getGoddardCacheDir } from "@goddard-ai/paths/node"
import type { LanguageModel } from "ai"
import {
  AdapterConfigurationError,
  InvalidProviderModuleError,
  MissingProviderPackageError,
  MissingTemplateVariableError,
  type ResolveTextModelLoadPlanOptions,
  type ResolvedTextModelModule,
  type TextModelConfig,
  type TextModelDescriptor,
  type TextModelLoadArgument,
  type TextModelLoadPlan,
  UnknownModelError,
  UnknownProviderError,
  resolveTextModel,
  resolveTextModelLoadPlan,
} from "ai-sdk-json-schema"
import { mkdir, stat, writeFile } from "node:fs/promises"
import { createRequire } from "node:module"
import { join, resolve } from "node:path"

import { runCommand } from "../worktrees/process.ts"

export {
  AdapterConfigurationError,
  InvalidProviderModuleError,
  MissingProviderPackageError,
  MissingTemplateVariableError,
  UnknownModelError,
  UnknownProviderError,
} from "ai-sdk-json-schema"

/**
 * One daemon-managed dynamic import function for one supported provider module.
 */
export type DaemonTextModelModuleLoader = () => Promise<Record<string, unknown>>

/**
 * One provider-package installation request the daemon may fulfill before model loading.
 */
export type DaemonTextModelPackageInstallRequest = {
  config: TextModelConfig
  descriptor: TextModelDescriptor
  installationRoot: string
  packageName: string
  installPackageName: string
  specifier: string
}

/**
 * One daemon-owned hook for installing provider packages into an external installation root.
 */
export type DaemonTextModelPackageInstaller = (
  request: DaemonTextModelPackageInstallRequest,
) => Promise<void>

/**
 * One daemon-owned hook for resolving the concrete load plan used to build a text model.
 */
export type DaemonTextModelLoadPlanResolver = (request: {
  config: TextModelConfig
  descriptor: TextModelDescriptor
  installationRoot: string
  env: Record<string, string | undefined>
  packageOptions: unknown
}) => TextModelLoadPlan

/**
 * Runtime-only options used while resolving provider packages for daemon AI features.
 */
export interface DaemonTextModelResolveOptions extends ResolveTextModelLoadPlanOptions {
  installMissingPackage?: DaemonTextModelPackageInstaller | false
  moduleLoaders?: Partial<Record<string, DaemonTextModelModuleLoader>>
  resolveLoadPlan?: DaemonTextModelLoadPlanResolver
}

/**
 * One ready-to-use text model resolved from shared JSON config.
 */
export type LoadedDaemonTextModel = {
  config: TextModelConfig
  descriptor: TextModelDescriptor
  model: LanguageModel
}

const require = createRequire(import.meta.url)
const bundledProviderInstallationRoot = resolve(import.meta.dirname, "../..")
const daemonProviderInstallationRoot = resolve(getGoddardCacheDir(), "text-model-providers")
const daemonProviderRootManifest = {
  name: "@goddard-ai/text-model-providers",
  private: true,
}
const daemonPackageManagers = [
  { command: "bun", args: ["add", "--exact"] },
  { command: "npm", args: ["install", "--save-exact", "--no-audit", "--no-fund"] },
  { command: "pnpm", args: ["add", "--save-exact"] },
  { command: "yarn", args: ["add", "--exact"] },
] as const
const defaultDaemonTextModelModuleLoaders = {
  "@ai-sdk/anthropic": async () => (await import("@ai-sdk/anthropic")) as Record<string, unknown>,
  "@ai-sdk/google": async () => (await import("@ai-sdk/google")) as Record<string, unknown>,
  "@ai-sdk/openai": async () => (await import("@ai-sdk/openai")) as Record<string, unknown>,
  "@ai-sdk/openai-compatible": async () =>
    (await import("@ai-sdk/openai-compatible")) as Record<string, unknown>,
  "@openrouter/ai-sdk-provider": async () =>
    (await import("@openrouter/ai-sdk-provider")) as Record<string, unknown>,
} satisfies Record<string, DaemonTextModelModuleLoader>

/** Returns the installable package name for a module specifier that may include one subpath. */
function getInstallPackageName(specifier: string) {
  if (specifier.startsWith("@")) {
    const [scope, name] = specifier.split("/", 3)
    return scope && name ? `${scope}/${name}` : specifier
  }

  const [name] = specifier.split("/", 2)
  return name || specifier
}

/** Returns true when the daemon ships one module loader for the resolved provider package. */
function isBundledProviderPackage(packageName: string) {
  return packageName in defaultDaemonTextModelModuleLoaders
}

/** Ensures one daemon-owned provider installation root is ready for external package installs. */
async function ensureDaemonProviderInstallationRoot(installationRoot: string) {
  await mkdir(installationRoot, { recursive: true })

  const manifestPath = join(installationRoot, "package.json")
  try {
    await stat(manifestPath)
  } catch {
    await writeFile(
      manifestPath,
      `${JSON.stringify(daemonProviderRootManifest, null, 2)}\n`,
      "utf8",
    )
  }
}

/** Installs one missing provider package into the daemon-managed external package root. */
export async function installDaemonTextModelPackage(request: DaemonTextModelPackageInstallRequest) {
  await ensureDaemonProviderInstallationRoot(request.installationRoot)

  const packageManager = daemonPackageManagers.find(({ command }) => Bun.which(command))
  if (!packageManager) {
    throw new Error(
      `No supported package manager is available to install "${request.installPackageName}".`,
    )
  }

  const result = await runCommand(
    packageManager.command,
    [...packageManager.args, request.installPackageName],
    {
      cwd: request.installationRoot,
      stdin: "ignore",
    },
  )

  if (result.status !== 0) {
    throw new Error(
      result.stderr.trim() ||
        result.stdout.trim() ||
        `Installing "${request.installPackageName}" exited unsuccessfully.`,
    )
  }
}

/** Loads one resolved provider module, preferring daemon-owned lazy imports when available. */
async function loadResolvedTextModelModule(
  module: ResolvedTextModelModule,
  moduleLoaders: Partial<Record<string, DaemonTextModelModuleLoader>>,
) {
  const customLoader = moduleLoaders[module.specifier]
  if (customLoader) {
    return customLoader()
  }

  const loaded = await import(module.fileUrl)
  if (module.exportName in loaded) {
    return loaded as Record<string, unknown>
  }

  const required = require(module.resolvedPath)
  if (required && typeof required === "object" && module.exportName in required) {
    return required as Record<string, unknown>
  }

  if (
    required &&
    typeof required === "object" &&
    "default" in required &&
    required.default &&
    typeof required.default === "object" &&
    module.exportName in required.default
  ) {
    return required.default as Record<string, unknown>
  }

  throw new InvalidProviderModuleError({
    specifier: module.specifier,
    resolvedPath: module.resolvedPath,
    exportName: module.exportName,
  })
}

/** Resolves one literal or binding-backed operation argument from the current execution state. */
function resolveLoadArgument(bindings: Map<string, unknown>, argument: TextModelLoadArgument) {
  return argument.kind === "value" ? argument.value : bindings.get(argument.binding)
}

/** Verifies that one loaded export can be called as part of the text-model load plan. */
function getCallableExport(value: unknown, specifier: string, exportName: string) {
  if (typeof value === "function") {
    return value
  }

  throw new InvalidProviderModuleError({
    specifier,
    resolvedPath: specifier,
    exportName,
  })
}

/** Executes one resolved load plan with daemon-owned module loading rules. */
async function executeTextModelLoadPlan(
  plan: TextModelLoadPlan,
  moduleLoaders: Partial<Record<string, DaemonTextModelModuleLoader>>,
) {
  const modulesByRole = new Map(plan.modules.map((module) => [module.role, module]))
  const exportsByRole = new Map<string, Record<string, unknown>>()

  for (const module of plan.modules) {
    exportsByRole.set(module.role, await loadResolvedTextModelModule(module, moduleLoaders))
  }

  const bindings = new Map<string, unknown>()
  for (const operation of plan.operations) {
    if (operation.kind === "create-binding") {
      const module = modulesByRole.get(operation.moduleRole)
      const loaded = exportsByRole.get(operation.moduleRole)
      if (!module || !loaded) {
        throw new InvalidProviderModuleError({
          specifier: operation.moduleRole,
          resolvedPath: operation.moduleRole,
          exportName: operation.moduleRole,
        })
      }

      const exportedValue = loaded[module.exportName]
      const callable = getCallableExport(exportedValue, module.specifier, module.exportName)
      bindings.set(operation.binding, callable(operation.options))
      continue
    }

    const target = bindings.get(operation.targetBinding)
    const args = operation.args.map((argument) => resolveLoadArgument(bindings, argument))
    if (operation.methodName) {
      if (!target || typeof target !== "object") {
        throw new InvalidProviderModuleError({
          specifier: operation.targetBinding,
          resolvedPath: operation.targetBinding,
          exportName: operation.methodName,
        })
      }

      const methodValue = (target as Record<string, unknown>)[operation.methodName]
      const callable = getCallableExport(methodValue, operation.targetBinding, operation.methodName)
      bindings.set(operation.binding, callable(...args))
      continue
    }

    const callable = getCallableExport(target, operation.targetBinding, operation.targetBinding)
    bindings.set(operation.binding, callable(...args))
  }

  return bindings.get(plan.resultBinding)
}

/** Resolves the load plan root and installs one missing external package when daemon policy allows it. */
async function resolveDaemonTextModelLoadPlan(
  config: TextModelConfig,
  descriptor: TextModelDescriptor,
  options: DaemonTextModelResolveOptions,
) {
  const installationRoot = isBundledProviderPackage(descriptor.packageName)
    ? bundledProviderInstallationRoot
    : resolve(options.installationRoot ?? daemonProviderInstallationRoot)
  const env = options.env ?? process.env
  const resolveLoadPlan =
    options.resolveLoadPlan ??
    ((request: {
      config: TextModelConfig
      descriptor: TextModelDescriptor
      installationRoot: string
      env: Record<string, string | undefined>
      packageOptions: unknown
    }) =>
      resolveTextModelLoadPlan(request.config, {
        installationRoot: request.installationRoot,
        env: request.env,
        packageOptions: request.packageOptions,
      }))

  try {
    return resolveLoadPlan({
      installationRoot,
      config,
      descriptor,
      env,
      packageOptions: options.packageOptions,
    })
  } catch (error) {
    if (
      isBundledProviderPackage(descriptor.packageName) ||
      !(error instanceof MissingProviderPackageError)
    ) {
      throw error
    }

    const installMissingPackage =
      options.installMissingPackage === undefined
        ? installDaemonTextModelPackage
        : options.installMissingPackage
    if (!installMissingPackage) {
      throw error
    }

    await ensureDaemonProviderInstallationRoot(installationRoot)
    const request = {
      config,
      descriptor,
      installationRoot,
      packageName: error.packageName,
      installPackageName: getInstallPackageName(error.specifier),
      specifier: error.specifier,
    } satisfies DaemonTextModelPackageInstallRequest

    await installMissingPackage(request)
    return resolveLoadPlan({
      installationRoot,
      config,
      descriptor,
      env,
      packageOptions: options.packageOptions,
    })
  }
}

/** Loads a runtime AI SDK language model from one persisted text-model config. */
export async function loadDaemonTextModel(
  config: unknown,
  options: DaemonTextModelResolveOptions = {},
) {
  const descriptor = resolveTextModel(config)
  const normalizedConfig: TextModelConfig = {
    provider: descriptor.provider,
    model: descriptor.model,
  }
  const plan = await resolveDaemonTextModelLoadPlan(normalizedConfig, descriptor, options)
  const model = (await executeTextModelLoadPlan(plan, {
    ...defaultDaemonTextModelModuleLoaders,
    ...options.moduleLoaders,
  })) as LanguageModel

  return {
    config: normalizedConfig,
    descriptor,
    model,
  }
}
