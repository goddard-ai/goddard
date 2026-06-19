import { expect, test } from "vitest"

import { filterPreparedSettingsSearchSections, prepareSettingsSearchSections } from "./search.ts"

const sections = [
  {
    id: "appearance-title",
    title: "Appearance",
    description: "Choose visual preferences.",
  },
  {
    id: "appearance-description",
    title: "Visual preferences",
    description: "Appearance controls for the workbench.",
  },
  {
    id: "shortcuts",
    title: "Shortcuts",
    description: "Manage keyboard commands.",
  },
]

test("settings search filters sections by fuzzy title and description matches", () => {
  const preparedSections = prepareSettingsSearchSections(sections)

  expect(filterPreparedSettingsSearchSections(preparedSections, "shrt")).toEqual([sections[2]])
  expect(filterPreparedSettingsSearchSections(preparedSections, "wrkbnch")).toEqual([sections[1]])
})

test("settings search ranks title matches ahead of description matches", () => {
  const preparedSections = prepareSettingsSearchSections(sections)

  expect(filterPreparedSettingsSearchSections(preparedSections, "appearance")).toEqual([
    sections[0],
    sections[1],
  ])
})
