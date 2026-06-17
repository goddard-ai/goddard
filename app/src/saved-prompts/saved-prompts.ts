import { Sigma } from "preact-sigma"

/** One prompt bookmarked from a transcript for reuse in session inputs. */
export type SavedPromptRecord = {
  id: string
  text: string
  savedAt: number
}

export type SubmittedPromptRecord = {
  text: string
  submittedAts: number[]
}

/** Public state for saved transcript prompts. */
export type SavedPromptsState = {
  promptsById: Record<string, SavedPromptRecord>
  orderedPromptIds: string[]
  submittedPromptsByText: Record<string, SubmittedPromptRecord>
}

const AUTO_SAVE_PROMPT_REPEAT_COUNT = 3
const AUTO_SAVE_PROMPT_WINDOW_MS = 7 * 24 * 60 * 60 * 1000

function normalizePromptText(text: string) {
  return text.trim()
}

function createSubmittedPromptKey(text: string) {
  return JSON.stringify([text])
}

function createSavedPromptId() {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID()
  }

  return `saved_prompt_${Date.now().toString(36)}_${Math.random().toString(36).slice(2)}`
}

/** Owns app-local saved prompt persistence and prefix lookup. */
export class SavedPrompts extends Sigma<SavedPromptsState> {
  constructor() {
    super({
      promptsById: {},
      orderedPromptIds: [],
      submittedPromptsByText: {},
    })
  }

  /** Returns saved prompts with the most recently saved prompt first. */
  get promptList() {
    return this.orderedPromptIds
      .map((promptId) => this.promptsById[promptId])
      .filter((prompt): prompt is SavedPromptRecord => Boolean(prompt))
      .sort((left, right) => right.savedAt - left.savedAt)
  }

  findByText(text: string) {
    const normalizedText = normalizePromptText(text)

    if (!normalizedText) {
      return null
    }

    return this.promptList.find((prompt) => prompt.text === normalizedText) ?? null
  }

  isSaved(text: string) {
    return this.findByText(text) !== null
  }

  save(text: string) {
    const normalizedText = normalizePromptText(text)

    if (!normalizedText) {
      return null
    }

    const existingPrompt = this.findByText(normalizedText)
    const prompt = {
      id: existingPrompt?.id ?? createSavedPromptId(),
      text: normalizedText,
      savedAt: Date.now(),
    }

    this.promptsById[prompt.id] = prompt
    this.orderedPromptIds = [
      prompt.id,
      ...this.orderedPromptIds.filter((promptId) => promptId !== prompt.id),
    ]

    return prompt
  }

  remove(id: string) {
    delete this.promptsById[id]
    this.orderedPromptIds = this.orderedPromptIds.filter((promptId) => promptId !== id)
  }

  toggle(text: string) {
    const existingPrompt = this.findByText(text)

    if (existingPrompt) {
      this.remove(existingPrompt.id)
      return null
    }

    return this.save(text)
  }

  recordSubmission(text: string, submittedAt = Date.now()) {
    const normalizedText = normalizePromptText(text)

    if (!normalizedText) {
      return null
    }

    const promptKey = createSubmittedPromptKey(normalizedText)
    const repeatWindowStart = submittedAt - AUTO_SAVE_PROMPT_WINDOW_MS
    const submittedPromptsByText: Record<string, SubmittedPromptRecord> = {}

    for (const [existingPromptKey, prompt] of Object.entries(this.submittedPromptsByText ?? {})) {
      const submittedAts = prompt.submittedAts.filter(
        (previousSubmittedAt) => previousSubmittedAt >= repeatWindowStart,
      )

      if (submittedAts.length > 0) {
        submittedPromptsByText[existingPromptKey] = {
          text: prompt.text,
          submittedAts,
        }
      }
    }

    const submittedAts = [...(submittedPromptsByText[promptKey]?.submittedAts ?? []), submittedAt]

    this.submittedPromptsByText = {
      ...submittedPromptsByText,
      [promptKey]: {
        text: normalizedText,
        submittedAts,
      },
    }

    if (submittedAts.length >= AUTO_SAVE_PROMPT_REPEAT_COUNT && !this.isSaved(normalizedText)) {
      return this.save(normalizedText)
    }

    return this.findByText(normalizedText)
  }

  findCompletion(prefix: string) {
    if (!prefix) {
      return null
    }

    return (
      this.promptList.find((prompt) => prompt.text !== prefix && prompt.text.startsWith(prefix)) ??
      null
    )
  }
}

export interface SavedPrompts extends SavedPromptsState {}
