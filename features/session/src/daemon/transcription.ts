/** Daemon-owned transcription runtime that prefers dedicated transcription APIs and falls back to text file prompts. */
import { generateText, experimental_transcribe as transcribe } from "ai"
import type { ModelConfig } from "ai-sdk-json-schema"

import type { TranscriptionAudioInput } from "../schema.ts"
import { loadDaemonTextModel, loadDaemonTranscriptionModel } from "./transcription-model-loader.ts"

const transcriptionFallbackPrompt =
  "Transcribe the spoken audio verbatim. Return only the transcript text. Do not summarize, translate, add speaker labels, add timestamps, or use markdown."

/** Converts one shared transcription input into the AI SDK audio argument. */
function toAiSdkAudioInput(audio: TranscriptionAudioInput) {
  return audio.type === "url" ? new URL(audio.url) : audio.data
}

/** Converts one shared transcription input into a text-model file prompt part. */
function toTextPromptFilePart(audio: TranscriptionAudioInput) {
  return {
    type: "file" as const,
    data: toAiSdkAudioInput(audio),
    mediaType: audio.mediaType,
    filename: audio.filename,
  }
}

/** Rejects empty transcription output from either runtime path. */
function requireTranscriptionText(text: string, label: string) {
  const normalizedText = text.trim()
  if (normalizedText.length === 0) {
    throw new Error(`${label} returned no transcript text.`)
  }

  return normalizedText
}

/** Attempts one text-model file-attachment fallback for transcription. */
async function transcribeWithTextFallback(config: ModelConfig, audio: TranscriptionAudioInput) {
  const loadedTextModel = await loadDaemonTextModel(config)
  const result = await generateText({
    model: loadedTextModel.model,
    messages: [
      {
        role: "user",
        content: [{ type: "text", text: transcriptionFallbackPrompt }, toTextPromptFilePart(audio)],
      },
    ],
  })

  return {
    text: requireTranscriptionText(result.text, "Text-model transcription fallback"),
  }
}

/**
 * Transcribes one audio payload using the configured provider, preferring dedicated transcription APIs.
 */
export async function transcribeAudioWithDaemonModel(params: {
  config: ModelConfig
  audio: TranscriptionAudioInput
}) {
  const loadedTranscriptionModel = await loadDaemonTranscriptionModel(params.config)
  const supportsDedicatedTranscription =
    loadedTranscriptionModel.descriptor.supportedLoadModes.includes("transcription")
  const supportsTextFallback =
    loadedTranscriptionModel.descriptor.supportedLoadModes.includes("text")

  if (supportsDedicatedTranscription) {
    try {
      const result = await transcribe({
        model: loadedTranscriptionModel.model,
        audio: toAiSdkAudioInput(params.audio),
      })

      return {
        text: requireTranscriptionText(result.text, "Dedicated transcription"),
      }
    } catch (transcriptionError) {
      if (!supportsTextFallback) {
        throw new Error(
          `Transcription failed for "${loadedTranscriptionModel.descriptor.provider}/${loadedTranscriptionModel.descriptor.model}", and the configured provider package does not support text-mode fallback. ${
            transcriptionError instanceof Error
              ? transcriptionError.message
              : String(transcriptionError)
          }`,
          { cause: transcriptionError },
        )
      }

      try {
        return await transcribeWithTextFallback(params.config, params.audio)
      } catch (fallbackError) {
        throw new Error(
          `Transcription failed for "${loadedTranscriptionModel.descriptor.provider}/${loadedTranscriptionModel.descriptor.model}" using both the dedicated transcription API and text-file fallback. ${
            fallbackError instanceof Error ? fallbackError.message : String(fallbackError)
          }`,
          { cause: fallbackError },
        )
      }
    }
  }

  if (!supportsTextFallback) {
    throw new Error(
      `Configured transcription model "${loadedTranscriptionModel.descriptor.provider}/${loadedTranscriptionModel.descriptor.model}" does not support the AI SDK transcription API or text-file fallback.`,
    )
  }

  try {
    return await transcribeWithTextFallback(params.config, params.audio)
  } catch (fallbackError) {
    throw new Error(
      `Transcription failed for "${loadedTranscriptionModel.descriptor.provider}/${loadedTranscriptionModel.descriptor.model}" using text-file fallback. ${
        fallbackError instanceof Error ? fallbackError.message : String(fallbackError)
      }`,
      { cause: fallbackError },
    )
  }
}
