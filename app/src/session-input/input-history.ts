type PromptBlock = {
  type: string
  text?: unknown
  [key: string]: unknown
}

/** Extracts only user-visible text blocks for prompt history substring matching. */
export function getSessionInputPromptText(blocks: readonly PromptBlock[]) {
  return blocks
    .flatMap((block) =>
      block.type === "text" && typeof block.text === "string" ? [block.text] : [],
    )
    .join("")
}

/** Finds prompt history entries whose visible text contains one fixed case-insensitive filter. */
export function getSessionInputPromptHistoryIndexes(
  promptHistory: readonly (readonly PromptBlock[])[],
  filter: string,
) {
  const normalizedFilter = filter.trim().toLowerCase()

  if (normalizedFilter.length === 0) {
    return promptHistory.map((_, index) => index)
  }

  return promptHistory.flatMap((prompt, index) =>
    getSessionInputPromptText(prompt).toLowerCase().includes(normalizedFilter) ? [index] : [],
  )
}
