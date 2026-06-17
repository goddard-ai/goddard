/** Converts an English source phrase into the canonical lower-camel language key. */
export function toLanguageKey(phrase: string) {
  const words = phrase
    .normalize("NFKD")
    .replace(/['’]/g, "")
    .replace(/[^A-Za-z0-9]+/g, " ")
    .trim()
    .split(/\s+/)
    .filter(Boolean)

  return words
    .map((word, index) => {
      const normalizedWord = word.toLowerCase()

      if (index === 0) {
        return normalizedWord
      }

      return normalizedWord.charAt(0).toUpperCase() + normalizedWord.slice(1)
    })
    .join("")
}
