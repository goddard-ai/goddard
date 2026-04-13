import * as acp from "@agentclientprotocol/sdk"
import type { InitialPromptOption } from "@goddard-ai/schema/daemon"
import type { DaemonSessionTitleState } from "@goddard-ai/schema/daemon/store"
import { generateText, type LanguageModel } from "ai"
import type { TextModelConfig } from "ai-sdk-json-schema"

const DEFAULT_SESSION_TITLE = "New session"
const MAX_FALLBACK_TITLE_WORDS = 6
const MAX_SESSION_TITLE_LENGTH = 48
const MIN_GENERATED_TITLE_WORDS = 2
const MAX_GENERATED_TITLE_WORDS = 6
const DAEMON_SYSTEM_PROMPT_PREFIX = '<system-prompt name="Goddard CLI">'
const DAEMON_SYSTEM_PROMPT_SUFFIX = "</system-prompt>"

/**
 * One prepared session-title payload derived from prompt text and config state.
 */
export type PreparedSessionTitle = {
  title: string
  titleState: Extract<DaemonSessionTitleState, "placeholder" | "fallback" | "pending">
  promptText: string | null
  generatorConfig: TextModelConfig | undefined
}

/** Returns true when one ACP content block carries plain text. */
function isTextContentBlock(block: acp.ContentBlock): block is acp.ContentBlock & { text: string } {
  return block.type === "text" && typeof (block as { text?: unknown }).text === "string"
}

/** Collapses multi-line or repeated whitespace into one single-space-separated string. */
function collapseWhitespace(text: string) {
  return text.replace(/\s+/g, " ").trim()
}

/** Drops the daemon-owned system-prompt wrapper from persisted history prompt extraction. */
function stripDaemonSystemPrompt(text: string) {
  return text.startsWith(DAEMON_SYSTEM_PROMPT_PREFIX) && text.endsWith(DAEMON_SYSTEM_PROMPT_SUFFIX)
    ? ""
    : text
}

/** Removes trailing punctuation that makes compact titles look like unfinished sentences. */
function stripTrailingTitlePunctuation(text: string) {
  return text.replace(/[.,:;!?]+$/u, "").trim()
}

/** Counts human-readable words after normalization. */
function countWords(text: string) {
  return text.split(/\s+/u).filter(Boolean).length
}

/** Returns true when one generated title still looks like prompt text or formatting noise. */
function looksPromptLike(text: string) {
  return (
    /^(user|assistant|system)\s*:/iu.test(text) || text.includes("```") || /[*_#[\]`]/u.test(text)
  )
}

/** Extracts normalized user prompt text from one initial prompt payload. */
export function extractSessionPromptText(
  prompt: InitialPromptOption | readonly acp.ContentBlock[],
  options: {
    skipDaemonSystemPrompt?: boolean
  } = {},
) {
  const blocks =
    typeof prompt === "string"
      ? ([{ type: "text", text: prompt }] satisfies acp.ContentBlock[])
      : prompt
  const text = collapseWhitespace(
    blocks
      .filter(isTextContentBlock)
      .map((block) =>
        options.skipDaemonSystemPrompt ? stripDaemonSystemPrompt(block.text) : block.text,
      )
      .join(" "),
  )

  return text.length > 0 ? text : null
}

/** Builds the deterministic local fallback title from the first user prompt text. */
export function createFallbackSessionTitle(promptText: string) {
  const normalizedPrompt = collapseWhitespace(promptText)
  if (normalizedPrompt.length === 0) {
    return null
  }

  const limitedWords = normalizedPrompt.split(" ").slice(0, MAX_FALLBACK_TITLE_WORDS)
  let fallbackTitle = ""

  for (const word of limitedWords) {
    const nextTitle = fallbackTitle.length === 0 ? word : `${fallbackTitle} ${word}`
    if (nextTitle.length > MAX_SESSION_TITLE_LENGTH) {
      break
    }

    fallbackTitle = nextTitle
  }

  if (fallbackTitle.length === 0) {
    fallbackTitle = normalizedPrompt.slice(0, MAX_SESSION_TITLE_LENGTH).trim()
  }

  fallbackTitle = stripTrailingTitlePunctuation(fallbackTitle)
  return fallbackTitle.length > 0 ? fallbackTitle : null
}

/** Computes the immediately visible title and pending state for one session prompt. */
export function prepareSessionTitle(
  prompt: InitialPromptOption | readonly acp.ContentBlock[] | undefined,
  generatorConfig?: TextModelConfig,
): PreparedSessionTitle {
  if (prompt === undefined) {
    return {
      title: DEFAULT_SESSION_TITLE,
      titleState: "placeholder",
      promptText: null,
      generatorConfig: undefined,
    }
  }

  const promptText = extractSessionPromptText(prompt)
  const fallbackTitle = promptText ? createFallbackSessionTitle(promptText) : null
  if (!promptText || !fallbackTitle) {
    return {
      title: DEFAULT_SESSION_TITLE,
      titleState: "placeholder",
      promptText: null,
      generatorConfig: undefined,
    }
  }

  return {
    title: fallbackTitle,
    titleState: generatorConfig ? "pending" : "fallback",
    promptText,
    generatorConfig,
  }
}

/** Finds the first persisted prompt request in session history and extracts its user text. */
export function extractFirstSessionPromptText(history: readonly acp.AnyMessage[]) {
  for (const message of history) {
    if (
      "method" in message &&
      message.method === acp.AGENT_METHODS.session_prompt &&
      typeof message.params === "object" &&
      message.params !== null &&
      "prompt" in message.params &&
      Array.isArray(message.params.prompt)
    ) {
      return extractSessionPromptText(message.params.prompt, {
        skipDaemonSystemPrompt: true,
      })
    }
  }

  return null
}

/** Validates and normalizes one provider-generated title response before persistence. */
export function normalizeGeneratedSessionTitle(title: string) {
  const lineNormalizedTitle = title.replace(/\r\n?/gu, "\n").trim()
  if (
    lineNormalizedTitle.length === 0 ||
    lineNormalizedTitle.includes("\n") ||
    lineNormalizedTitle.length > MAX_SESSION_TITLE_LENGTH
  ) {
    return null
  }

  const normalizedTitle = collapseWhitespace(lineNormalizedTitle)
  if (/^["'`].*["'`]$/u.test(normalizedTitle) || looksPromptLike(normalizedTitle)) {
    return null
  }

  const strippedTitle = stripTrailingTitlePunctuation(normalizedTitle)
  const wordCount = countWords(strippedTitle)
  if (
    strippedTitle.length === 0 ||
    wordCount < MIN_GENERATED_TITLE_WORDS ||
    wordCount > MAX_GENERATED_TITLE_WORDS
  ) {
    return null
  }

  return strippedTitle
}

/** Runs one model-backed title-generation request and validates the single-line result. */
export async function generateSessionTitle(params: { model: LanguageModel; promptText: string }) {
  const result = await generateText({
    model: params.model,
    system:
      "Generate a short session title. Return exactly one line, 2 to 6 words, no quotes, no markdown, and no trailing punctuation.",
    prompt: `User request:\n${params.promptText}`,
    maxOutputTokens: 20,
  })

  return normalizeGeneratedSessionTitle(result.text)
}

/** Backfills one persisted session title from existing initiative or prompt history when possible. */
export function backfillSessionTitle(params: {
  title: string
  titleState: DaemonSessionTitleState
  initiative: string | null
  history: readonly acp.AnyMessage[]
}) {
  if (params.titleState === "pending") {
    return {
      title: params.title || DEFAULT_SESSION_TITLE,
      titleState: "failed" as const,
    }
  }

  if (params.titleState !== "placeholder" || params.title !== DEFAULT_SESSION_TITLE) {
    return null
  }

  const initiativeTitle = params.initiative ? createFallbackSessionTitle(params.initiative) : null
  if (initiativeTitle) {
    return {
      title: initiativeTitle,
      titleState: "fallback" as const,
    }
  }

  const promptText = extractFirstSessionPromptText(params.history)
  const promptTitle = promptText ? createFallbackSessionTitle(promptText) : null
  if (!promptTitle) {
    return null
  }

  return {
    title: promptTitle,
    titleState: "fallback" as const,
  }
}
