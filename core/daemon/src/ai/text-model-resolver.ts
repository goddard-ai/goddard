import type { LanguageModel } from "ai"
import {
  type ResolveTextModelLoadPlanOptions,
  type TextModelConfig,
  type TextModelDescriptor,
  loadTextModel,
  resolveTextModel,
} from "ai-sdk-json-schema"
import { resolve } from "node:path"

export {
  AdapterConfigurationError,
  InvalidProviderModuleError,
  MissingProviderPackageError,
  MissingTemplateVariableError,
  UnknownModelError,
  UnknownProviderError,
} from "ai-sdk-json-schema"

/**
 * Runtime-only options used while resolving provider packages for daemon AI features.
 */
export type DaemonTextModelResolveOptions = ResolveTextModelLoadPlanOptions

/**
 * One ready-to-use text model resolved from shared JSON config.
 */
export type LoadedDaemonTextModel = {
  config: TextModelConfig
  descriptor: TextModelDescriptor
  model: LanguageModel
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
  const model = (await loadTextModel(normalizedConfig, {
    installationRoot: options.installationRoot ?? resolve(import.meta.dirname, "../.."),
    env: options.env ?? process.env,
    packageOptions: options.packageOptions,
  })) as LanguageModel

  return {
    config: normalizedConfig,
    descriptor,
    model,
  }
}
