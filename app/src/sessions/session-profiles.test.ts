import { createFixtureModelConfigOption } from "@goddard-ai/fixtures"
import { deriveSessionProfileConfig, type AgentSessionProfiles } from "@goddard-ai/sdk"
import { expect, test } from "vitest"

import { findMatchingSessionProfileId, getAvailableSessionProfiles } from "./session-profiles.ts"

function createConfig() {
  return deriveSessionProfileConfig({
    configOptions: [
      createFixtureModelConfigOption({
        currentValue: "sonnet",
        models: [
          { modelId: "haiku", name: "Haiku" },
          { modelId: "sonnet", name: "Sonnet" },
        ],
      }),
      {
        id: "thought_level",
        type: "select",
        name: "Thinking level",
        category: "thought_level",
        currentValue: "medium",
        options: [
          { value: "low", name: "Low" },
          { value: "medium", name: "Medium" },
        ],
      },
      {
        id: "mode",
        type: "select",
        name: "Approval mode",
        category: "mode",
        currentValue: "default",
        options: [
          { value: "default", name: "Default" },
          { value: "full-auto", name: "Full auto" },
        ],
      },
    ],
  })
}

const profiles = {
  routine: {
    model: "haiku",
    thoughtLevel: "low",
    approvalMode: "full-auto",
  },
  debug: {
    model: "sonnet",
    thoughtLevel: "medium",
    approvalMode: "default",
  },
  deep: {
    model: "removed-model",
    thoughtLevel: "medium",
    approvalMode: "default",
  },
} satisfies AgentSessionProfiles

test("session profile choices omit stale profiles", () => {
  expect(
    getAvailableSessionProfiles(profiles, createConfig()).map(({ profileId }) => profileId),
  ).toEqual(["routine", "debug"])
})

test("session profile choices omit unconfigured slots", () => {
  expect(
    getAvailableSessionProfiles({ routine: profiles.routine }, createConfig()).map(
      ({ profileId }) => profileId,
    ),
  ).toEqual(["routine"])
})

test("matching profile is derived from the complete effective selection", () => {
  const config = createConfig()

  expect(findMatchingSessionProfileId(profiles, config)).toBe("debug")
  expect(
    findMatchingSessionProfileId(profiles, config, {
      modelId: "haiku",
      thinkingValue: "low",
      approvalModeValue: "full-auto",
    }),
  ).toBe("routine")
  expect(
    findMatchingSessionProfileId(profiles, config, {
      modelId: "haiku",
      thinkingValue: "medium",
      approvalModeValue: "full-auto",
    }),
  ).toBeNull()
})
