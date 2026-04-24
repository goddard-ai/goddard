/** Returns whether one search query should be treated as empty by local UI filters. */
export function isEmptyQuery(query: string) {
  return query.trimStart().length === 0
}
