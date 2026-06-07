import { expect, test } from "bun:test"

import { SelectorUsage, SelectorUsageKey } from "~/selector-usage.ts"
import {
  recordSessionLaunchUse,
  resolvePreferredLaunchAgentId,
  resolvePreferredLaunchCwd,
  setCurrentSessionLaunchAgent,
  setCurrentSessionLaunchCwd,
} from "./launch-preferences.ts"

test("launch preferences resolve current values before MRU and defaults", () => {
  const selectorUsage = new SelectorUsage()
  const adapterCatalog = {
    adapters: [
      createAdapter("codex", "Codex"),
      createAdapter("claude", "Claude"),
      createAdapter("pi", "Pi"),
    ],
    defaultAdapterId: "codex",
    lastError: null,
    lastSuccessfulSyncAt: null,
    registrySource: "cache" as const,
    stale: false,
  }

  recordSessionLaunchUse(selectorUsage, {
    agentId: "claude",
    branchName: "main",
    cwd: "/repo/packages/app",
    location: "worktree",
    modeValue: "on-request",
    modelId: "opus",
    projectPath: "/repo",
    repoRoot: "/repo",
    thinkingValue: "high",
  })
  setCurrentSessionLaunchAgent(selectorUsage, "pi")
  setCurrentSessionLaunchCwd(selectorUsage, "/repo", "/repo")

  expect(resolvePreferredLaunchAgentId(selectorUsage, adapterCatalog)).toBe("pi")
  expect(resolvePreferredLaunchCwd(selectorUsage, "/repo", [{ path: "/repo/packages/app" }])).toBe(
    "/repo",
  )
})

function createAdapter(id: string, name: string) {
  return {
    id,
    name,
    version: "1.0.0",
    description: `${name} adapter`,
    distribution: { npx: { package: id } },
    unofficial: false,
    source: "registry" as const,
  }
}

test("recordSessionLaunchUse records shared session control keys per agent", () => {
  const selectorUsage = new SelectorUsage()

  recordSessionLaunchUse(selectorUsage, {
    agentId: "codex",
    branchName: "main",
    cwd: "/repo",
    location: "local",
    modeValue: "never",
    modelId: "gpt-5",
    projectPath: "/repo",
    repoRoot: "/repo",
    thinkingValue: "medium",
  })

  expect(selectorUsage.getRecentUsedValues(SelectorUsageKey.sessionControlModel("codex"))).toEqual([
    "gpt-5",
  ])
  expect(selectorUsage.getRecentUsedValues(SelectorUsageKey.sessionControlMode("codex"))).toEqual([
    "never",
  ])
  expect(
    selectorUsage.getRecentUsedValues(SelectorUsageKey.sessionControlThinking("codex")),
  ).toEqual(["medium"])
})
