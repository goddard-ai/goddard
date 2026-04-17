import * as acp from "@agentclientprotocol/sdk"
import { expect, test } from "bun:test"

import {
  backfillSessionTitle,
  createFallbackSessionTitle,
  normalizeGeneratedSessionTitle,
  prepareSessionTitle,
} from "../src/session/title.ts"

test("prepareSessionTitle derives fallback and pending state from the first prompt", () => {
  const preparedTitle = prepareSessionTitle("Fix the session list title flicker on refresh.", {
    provider: "openai",
    model: "gpt-4.1-mini",
  })

  expect(preparedTitle).toMatchObject({
    title: "Fix the session list title flicker",
    titleState: "pending",
    promptText: "Fix the session list title flicker on refresh.",
    generatorConfig: {
      provider: "openai",
      model: "gpt-4.1-mini",
    },
  })
})

test("prepareSessionTitle leaves non-text prompts on the placeholder title", () => {
  const preparedTitle = prepareSessionTitle([
    {
      type: "image",
      data: "https://example.com/screenshot.png",
      mimeType: "image/png",
    } as acp.ContentBlock,
  ])

  expect(preparedTitle).toEqual({
    title: "New session",
    titleState: "placeholder",
    promptText: null,
    generatorConfig: undefined,
  })
})

test("normalizeGeneratedSessionTitle accepts a compact title and rejects prompt-like output", () => {
  expect(normalizeGeneratedSessionTitle("Session list flicker fix.")).toBe(
    "Session list flicker fix",
  )
  expect(normalizeGeneratedSessionTitle("```md\nSession title\n```")).toBeNull()
  expect(normalizeGeneratedSessionTitle("Fix the session list title flicker on refresh")).toBeNull()
})

test("backfillSessionTitle restores old titles from persisted prompt history and fails abandoned pending titles", () => {
  const history = [
    {
      jsonrpc: "2.0",
      method: acp.AGENT_METHODS.session_prompt,
      params: {
        sessionId: "acp-session-1",
        prompt: [
          {
            type: "text",
            text: '<system-prompt name="goddard">Keep responses short.</system-prompt>',
          },
          { type: "text", text: "Audit the loop retry policy for edge cases." },
        ],
      },
    } satisfies acp.AnyMessage,
  ]

  expect(
    backfillSessionTitle({
      title: "New session",
      titleState: "placeholder",
      initiative: null,
      history,
    }),
  ).toEqual({
    title: "Audit the loop retry policy for",
    titleState: "fallback",
  })

  expect(
    backfillSessionTitle({
      title: createFallbackSessionTitle("Investigate title generation failures.") ?? "New session",
      titleState: "pending",
      initiative: null,
      history: [],
    }),
  ).toEqual({
    title: "Investigate title generation failures",
    titleState: "failed",
  })
})
