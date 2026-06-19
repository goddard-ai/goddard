import { resolveModel } from "ai-sdk-json-schema"
import { beforeEach, expect, mock, test } from "bun:test"

import type {
  LoadedDaemonTextModel,
  LoadedDaemonTranscriptionModel,
} from "../src/daemon/text-model-resolver.ts"

const sampleBase64Audio = {
  type: "base64" as const,
  data: "ZmFrZS1hdWRpby1ieXRlcw==",
  mediaType: "audio/mpeg",
  filename: "sample.mp3",
}

type GenerateTextCall = (params: {
  model: unknown
  messages: Array<{
    role: "user"
    content: Array<
      | { type: "text"; text: string }
      | {
          type: "file"
          data: string | URL
          mediaType: string
          filename?: string
        }
    >
  }>
}) => Promise<{ text: string }>

type LoadTextModelCall = (config: unknown) => Promise<LoadedDaemonTextModel>
type LoadTranscriptionModelCall = (config: unknown) => Promise<LoadedDaemonTranscriptionModel>
type TranscribeCall = (params: { model: unknown; audio: string | URL }) => Promise<{ text: string }>

const generateTextMock = mock<GenerateTextCall>()
const loadTextModelMock = mock<LoadTextModelCall>()
const loadTranscriptionModelMock = mock<LoadTranscriptionModelCall>()
const transcribeMock = mock<TranscribeCall>()

mock.module("ai", () => ({
  experimental_transcribe: transcribeMock,
  generateText: generateTextMock,
}))

mock.module(new URL("../src/daemon/transcription-model-loader.ts", import.meta.url).href, () => ({
  loadDaemonTextModel: loadTextModelMock,
  loadDaemonTranscriptionModel: loadTranscriptionModelMock,
}))

const { transcribeAudioWithDaemonModel } = await import(
  new URL("../src/daemon/transcription.ts", import.meta.url).href
)

beforeEach(() => {
  generateTextMock.mockReset()
  loadTextModelMock.mockReset()
  loadTranscriptionModelMock.mockReset()
  transcribeMock.mockReset()
})

test("transcribeAudioWithDaemonModel prefers the dedicated transcription api when available", async () => {
  const config = {
    provider: "openai",
    model: "whisper-1",
  } as const
  const descriptor = resolveModel("transcription", config)

  loadTranscriptionModelMock.mockResolvedValueOnce({
    descriptor,
    model: {} as never,
  })
  transcribeMock.mockImplementationOnce(
    async ({ audio }: { model: unknown; audio: string | URL }) => {
      expect(audio).toBe(sampleBase64Audio.data)
      return {
        text: "hello from dedicated transcription",
      }
    },
  )

  const result = await transcribeAudioWithDaemonModel({
    config,
    audio: sampleBase64Audio,
  })

  expect(result).toEqual({
    text: "hello from dedicated transcription",
  })
  expect(loadTextModelMock).not.toHaveBeenCalled()
  expect(generateTextMock).not.toHaveBeenCalled()
})

test("transcribeAudioWithDaemonModel falls back to text file prompts when dedicated transcription fails", async () => {
  const config = {
    provider: "openai",
    model: "whisper-1",
  } as const
  const transcriptionDescriptor = resolveModel("transcription", config)
  const textDescriptor = resolveModel("text", config)

  loadTranscriptionModelMock.mockResolvedValueOnce({
    descriptor: transcriptionDescriptor,
    model: {} as never,
  })
  transcribeMock.mockImplementationOnce(async () => {
    throw new Error("dedicated transcription unavailable")
  })
  loadTextModelMock.mockResolvedValueOnce({
    descriptor: textDescriptor,
    model: {} as never,
  })
  generateTextMock.mockImplementationOnce(async ({ messages }: Parameters<GenerateTextCall>[0]) => {
    expect(messages).toHaveLength(1)
    expect(messages[0]).toEqual({
      role: "user",
      content: [
        {
          type: "text",
          text: "Transcribe the spoken audio verbatim. Return only the transcript text. Do not summarize, translate, add speaker labels, add timestamps, or use markdown.",
        },
        {
          type: "file",
          data: new URL("https://example.com/audio.mp3"),
          mediaType: "audio/mpeg",
          filename: "audio.mp3",
        },
      ],
    })

    return {
      text: "hello from fallback transcription",
    }
  })

  const result = await transcribeAudioWithDaemonModel({
    config,
    audio: {
      type: "url",
      url: "https://example.com/audio.mp3",
      mediaType: "audio/mpeg",
      filename: "audio.mp3",
    },
  })

  expect(result).toEqual({
    text: "hello from fallback transcription",
  })
})
