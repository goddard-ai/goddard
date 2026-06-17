import { createWeaverPayload, type Emotion, type WeaverInput } from "./weaver.ts"

const emotions = ["grief", "dread", "awe", "tension", "calm", "obsession"] satisfies Emotion[]

export const creativeWeaverPipelineId = "creative-weaver"
export const creativeWeaverBuildPayloadTransformer = "creative-weaver.build-payload"

export const creativeWeaverScriptTransformers = {
  [creativeWeaverBuildPayloadTransformer]: ({ input }: { input: Record<string, unknown> }) =>
    createWeaverPayload(normalizeWeaverInput(input)),
}

function normalizeWeaverInput(input: Record<string, unknown>): WeaverInput {
  return {
    premise: parseStringInput(input, "premise"),
    emotion: parseEmotionInput(input.emotion ?? "tension"),
    seed: parseIntegerInput(input.seed ?? 1, "seed"),
    targetWords: parseIntegerInput(input.targetWords ?? input.words ?? 500, "targetWords"),
  }
}

function parseStringInput(input: Record<string, unknown>, key: string) {
  const value = input[key]
  if (typeof value !== "string" || value.trim().length === 0) {
    throw new Error(`Creative Weaver requires a non-empty "${key}" input.`)
  }

  return value
}

function parseEmotionInput(value: unknown) {
  if (typeof value !== "string" || !emotions.includes(value as Emotion)) {
    throw new Error(`Creative Weaver emotion must be one of: ${emotions.join(", ")}.`)
  }

  return value as Emotion
}

function parseIntegerInput(value: unknown, key: string) {
  const numberValue = typeof value === "string" ? Number.parseInt(value, 10) : value
  if (!Number.isInteger(numberValue) || String(numberValue) !== String(value)) {
    throw new Error(`Creative Weaver "${key}" input must be a whole number.`)
  }

  return numberValue as number
}
