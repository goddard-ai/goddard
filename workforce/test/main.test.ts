import { expect, test } from "vitest"
import { formatPackageLabel, resolveCommandMessage } from "../src/main.ts"

test("formatPackageLabel distinguishes the repository root from nested packages", () => {
  expect(
    formatPackageLabel({
      rootDir: "/repo",
      relativeDir: ".",
      manifestPath: "/repo/package.json",
      name: "@repo/root",
    }),
  ).toBe("@repo/root (repository root)")

  expect(
    formatPackageLabel({
      rootDir: "/repo/packages/ui",
      relativeDir: "packages/ui",
      manifestPath: "/repo/packages/ui/package.json",
      name: "@repo/ui",
    }),
  ).toBe("@repo/ui (packages/ui)")
})

test("resolveCommandMessage prefers the flag and rejects missing input", () => {
  expect(resolveCommandMessage({ positionalMessage: "Ship it." })).toBe("Ship it.")
  expect(
    resolveCommandMessage({
      positionalMessage: "Ignored",
      message: "Flag wins.",
    }),
  ).toBe("Flag wins.")
  expect(() => resolveCommandMessage({})).toThrow(
    "A message is required. Pass --message or provide it positionally.",
  )
})
