import { expect, test } from "vitest"

import { findActivationFocusTarget, findSearchFocusTarget } from "./focus-targets.ts"

test("search focus targets ignore activation-only controls", () => {
  const container = document.createElement("div")
  const activationTarget = document.createElement("div")
  activationTarget.contentEditable = "true"
  activationTarget.dataset.workbenchActivationFocus = "true"
  container.append(activationTarget)

  expect(findActivationFocusTarget(container)).toBe(activationTarget)
  expect(findSearchFocusTarget(container)).toBe(null)
})

test("search focus targets include search inputs and searchbox roles", () => {
  const container = document.createElement("div")
  const searchInput = document.createElement("input")
  searchInput.type = "search"
  const roleSearchbox = document.createElement("div")
  roleSearchbox.role = "searchbox"

  container.append(searchInput, roleSearchbox)

  expect(findSearchFocusTarget(container)).toBe(searchInput)

  searchInput.disabled = true

  expect(findSearchFocusTarget(container)).toBe(roleSearchbox)
})
