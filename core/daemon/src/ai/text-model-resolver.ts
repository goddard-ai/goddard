/** Daemon-owned text-model loading that prefers bundled provider packages and can install others on demand. */
import { getGoddardCacheDir } from "@goddard-ai/paths/node"
import type { LanguageModel } from "ai"
import {
  buildTextModelLoadPlan,
  executeTextModelLoadPlan,
  MissingProviderPackageError,
  type BuildTextModelLoadPlanOptions,
  type ResolveTextModelModulesOptions,
  type ResolvedTextModelLoadPlan,
  type TextModelConfig,
  type TextModelDescriptor,
  type UnresolvedTextModelLoadPlan,
  resolveTextModel,
  resolveTextModelModules,
} from "ai-sdk-json-schema"
import { mkdir, stat, writeFile } from "node:fs/promises"
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
 * One daemon-managed dynamic import function for one supported provider package.
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
 * Runtime-only options used while resolving provider packages for daemon AI features.
 */
export interface DaemonTextModelResolveOptions extends BuildTextModelLoadPlanOptions {
  installMissingPackage?: DaemonTextModelPackageInstaller | false
  moduleLoaders?: Partial<Record<string, DaemonTextModelModuleLoader>>
  installationRoot?: string
  /** Test-only override for module resolution after the daemon chooses an installation root. */
  resolveModules?: (
    plan: UnresolvedTextModelLoadPlan,
    options: ResolveTextModelModulesOptions,
  ) => ResolvedTextModelLoadPlan
}

/**
 * One ready-to-use text model resolved from shared JSON config.
 */
export type LoadedDaemonTextModel = {
  config: TextModelConfig
  descriptor: TextModelDescriptor
  model: LanguageModel
}

const daemonProviderInstallationRoot = resolve(getGoddardCacheDir(), "text-model-providers")
const daemonProviderRootManifest = {
  name: "@goddard-ai/text-model-providers",
  private: true,
}
const daemonPackageManagers = [
  { command: "bun", args: ["add", "--exact"] },
  { command: "pnpm", args: ["add", "--save-exact"] },
  { command: "npm", args: ["install", "--save-exact", "--no-audit", "--no-fund"] },
] as const
const defaultDaemonTextModelModuleLoaders = {
  "@ai-sdk/anthropic": () => import("@ai-sdk/anthropic"),
  "@ai-sdk/google": () => import("@ai-sdk/google"),
  "@ai-sdk/openai": () => import("@ai-sdk/openai"),
  "@ai-sdk/openai-compatible": () => import("@ai-sdk/openai-compatible"),
  "@openrouter/ai-sdk-provider": () => import("@openrouter/ai-sdk-provider"),
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

/** Returns true when one thrown value matches the missing-provider-package error contract. */
function isMissingProviderPackageResolutionError(
  error: unknown,
): error is MissingProviderPackageError {
  if (error instanceof MissingProviderPackageError) {
    return true
  }

  if (!(error instanceof Error)) {
    return false
  }

  const candidate = error as Error & {
    packageName?: unknown
    specifier?: unknown
    installationRoot?: unknown
  }
  return (
    typeof candidate.packageName === "string" &&
    typeof candidate.specifier === "string" &&
    typeof candidate.installationRoot === "string"
  )
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

/** Resolves one external-provider load plan and installs one missing package when daemon policy allows it. */
async function resolveDaemonTextModelModules(
  config: TextModelConfig,
  descriptor: TextModelDescriptor,
  plan: UnresolvedTextModelLoadPlan,
  options: DaemonTextModelResolveOptions,
) {
  const installationRoot = resolve(options.installationRoot ?? daemonProviderInstallationRoot)
  const resolveModules = options.resolveModules ?? resolveTextModelModules

  try {
    return resolveModules(plan, { installationRoot })
  } catch (error) {
    if (!isMissingProviderPackageResolutionError(error)) {
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
    return resolveModules(plan, { installationRoot })
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
  const plan = buildTextModelLoadPlan(normalizedConfig, {
    env: options.env ?? process.env,
    packageOptions: options.packageOptions,
  })

  if (isBundledProviderPackage(descriptor.packageName)) {
    const moduleLoaders: Record<string, DaemonTextModelModuleLoader> = {
      ...defaultDaemonTextModelModuleLoaders,
      ...options.moduleLoaders,
    }

    const model = (await executeTextModelLoadPlan(plan, {
      async loadModule(module) {
        const moduleLoader = moduleLoaders[module.packageName] ?? moduleLoaders[module.specifier]
        if (moduleLoader) {
          return moduleLoader()
        }

        throw new Error(
          `No daemon-owned loader is configured for bundled provider package "${module.packageName}".`,
        )
      },
    })) as LanguageModel

    return {
      config: normalizedConfig,
      descriptor,
      model,
    }
  }

  const model = (await executeTextModelLoadPlan(
    await resolveDaemonTextModelModules(normalizedConfig, descriptor, plan, options),
  )) as LanguageModel

  return {
    config: normalizedConfig,
    descriptor,
    model,
  }
}
