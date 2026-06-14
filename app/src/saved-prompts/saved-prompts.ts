import { Sigma } from "preact-sigma"

/** One prompt bookmarked from a transcript for reuse in session inputs. */
export type SavedPromptRecord = {
  id: string
  text: string
  savedAt: number
}

/** Public state for saved transcript prompts. */
export type SavedPromptsState = {
  promptsById: Record<string, SavedPromptRecord>
  orderedPromptIds: string[]
}

function normalizePromptText(text: string) {
  return text.trim()
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
