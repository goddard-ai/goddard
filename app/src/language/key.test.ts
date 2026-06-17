import { expect, test } from "bun:test"

import { englishText } from "./en.ts"
import { toLanguageKey } from "./key.ts"

test("language keys are lower camel case versions of English phrases", () => {
  expect(toLanguageKey("Open Next Unread Inbox Item")).toBe("openNextUnreadInboxItem")
  expect(toLanguageKey("Search settings")).toBe("searchSettings")
  expect(toLanguageKey("Couldn't open next item")).toBe("couldntOpenNextItem")
  expect(toLanguageKey("Try a different search term.")).toBe("tryADifferentSearchTerm")
})

test("language entries are accessed through lower camel case keys", () => {
  expect(Object.keys(englishText).every((key) => /^[a-z][A-Za-z0-9]*$/.test(key))).toBe(true)
})
