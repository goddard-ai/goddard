/** Resolves keyboard movement for a linear row list, wrapping at the list boundaries. */
export function getNextKeyboardRowIndex(input: {
  currentIndex: number | null
  direction: -1 | 1
  rowCount: number
}) {
  if (input.rowCount === 0) {
    return null
  }

  if (
    input.currentIndex === null ||
    input.currentIndex < 0 ||
    input.currentIndex >= input.rowCount
  ) {
    return input.direction > 0 ? 0 : input.rowCount - 1
  }

  return (input.currentIndex + input.direction + input.rowCount) % input.rowCount
}
