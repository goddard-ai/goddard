import * as fuzzysort from "fuzzysort2"

import { isEmptyQuery } from "~/lib/search-query.ts"

const settingsSearchFieldWeights: Record<string, number> = {
  description: 1,
  title: 2,
}

/** Prepares settings section title and description text for repeated filtering. */
export function prepareSettingsSearchSections<
  T extends {
    description: string
    title: string
  },
>(sections: readonly T[]) {
  return sections.map((section) => ({
    preparedDescription: fuzzysort.prepare(section.description),
    preparedTitle: fuzzysort.prepare(section.title),
    section,
  }))
}

/** Filters settings sections with title-weighted fuzzy matching. */
export function filterPreparedSettingsSearchSections<
  T extends {
    description: string
    title: string
  },
>(sections: ReturnType<typeof prepareSettingsSearchSections<T>>, searchQuery: string) {
  if (isEmptyQuery(searchQuery)) {
    return sections.map((entry) => entry.section)
  }

  return fuzzysort
    .searchFields(
      searchQuery,
      sections,
      [
        { key: "title", extract: (entry) => entry.preparedTitle },
        { key: "description", extract: (entry) => entry.preparedDescription },
      ],
      { threshold: 0 },
    )
    .items.map((item, index) => ({
      index,
      item,
      weightedScore: item.fields.reduce(
        (score, field) => score + field.score * (settingsSearchFieldWeights[field.key] ?? 1),
        0,
      ),
    }))
    .sort((left, right) => {
      const scoreDifference = right.weightedScore - left.weightedScore

      return scoreDifference === 0 ? left.index - right.index : scoreDifference
    })
    .map((entry) => entry.item.value.section)
}
