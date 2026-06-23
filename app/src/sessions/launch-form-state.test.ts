import { createFixtureModelConfigOption } from "@goddard-ai/fixtures"
import type { ListAgentsResponse, SessionLaunchPreviewResponse } from "@goddard-ai/sdk"
import { expect, test } from "vitest"

import { filterSlashCommandSuggestions, SessionLaunchFormState } from "./launch-form-state.ts"
import { preferredLaunchAgentId } from "./launch-preferences.ts"

function createManagedAgentCatalog(input: {
  agentIds: readonly string[]
  defaultAgentId: string | null
}): ListAgentsResponse {
  return {
    agents: input.agentIds.map((id) => ({
      id,
      name: id,
      version: "1.0.0",
      description: `${id} test managed agent`,
      distribution: { npx: { package: id } },
      source: "config",
      unofficial: false,
    })),
    installations: input.agentIds.map((id) => ({
      agentId: id,
      installed: true,
      installable: false,
      method: "config",
    })),
    defaultAgentId: input.defaultAgentId,
    registrySource: "cache",
    lastSuccessfulSyncAt: "2026-06-10T00:00:00.000Z",
    stale: false,
    lastError: null,
  }
}

function createLaunchPreview(
  input: Partial<SessionLaunchPreviewResponse> = {},
): SessionLaunchPreviewResponse {
  return {
    launchLeaseId: "lease_1",
    repoRoot: "/repo",
    bare: false,
    branches: ["main", "feature/testing"],
    currentBranch: "main",
    dirty: false,
    configOptions: [
      createFixtureModelConfigOption({
        currentValue: "sonnet",
        models: [
          {
            modelId: "sonnet",
            name: "Sonnet",
            description: "Balanced model",
          },
          {
            modelId: "opus",
            name: "Opus",
            description: "Deep reasoning model",
          },
        ],
      }),
      {
        id: "approval",
        type: "select",
        name: "Approval preset",
        category: "mode",
        currentValue: "default",
        options: [
          { value: "default", name: "Default" },
          { value: "plan", name: "Plan first" },
        ],
      },
      {
        id: "thought_level",
        type: "select",
        name: "Thinking level",
        category: "thought_level",
        currentValue: "medium",
        options: [
          { value: "medium", name: "Medium" },
          { value: "high", name: "High" },
        ],
      },
    ],
    slashCommands: [
      {
        type: "slash_command",
        name: "plan",
        description: "Create a plan",
        inputHint: "What should change?",
      },
      {
        type: "slash_command",
        name: "review",
        description: "Review code",
        inputHint: "What should be checked?",
      },
    ],
    ...input,
  }
}

test("SessionLaunchFormState builds a worktree launch request from selected controls", () => {
  preferredLaunchAgentId.value = null
  const form = new SessionLaunchFormState()

  form.managedAgentCatalog.value = createManagedAgentCatalog({
    agentIds: ["codex", "pi"],
    defaultAgentId: "pi",
  })
  form.draftProjectPath.value = "/repo"
  form.draftSubpackagePath.value = "/repo/packages/app"
  form.launchPreview.value = createLaunchPreview()
  form.setLaunchLocation("worktree")
  form.draftBaseBranchName.value = "feature/testing"
  form.draftModeValue.value = "plan"
  form.draftThinkingValue.value = "high"
  form.draftModelId.value = "opus"
  form.draftPromptBlocks.value = [{ type: "text", text: "Add stable inbox tests." }]

  expect(form.canSubmit.value).toBe(true)
  expect(form.sessionInput.value).toEqual({
    agent: "pi",
    cwd: "/repo/packages/app",
    worktree: {
      enabled: true,
      baseBranchName: "feature/testing",
    },
    mcpServers: [],
    initialModelId: undefined,
    initialConfigOptions: [
      {
        configId: "approval",
        value: "plan",
      },
      {
        configId: "thought_level",
        value: "high",
      },
      {
        configId: "model",
        value: "opus",
      },
    ],
    initialPrompt: [{ type: "text", text: "Add stable inbox tests." }],
    launchLeaseId: undefined,
    localCheckout: undefined,
  })
})

test("SessionLaunchFormState keeps launch leases only for unchanged local branch launches", () => {
  preferredLaunchAgentId.value = null
  const form = new SessionLaunchFormState()

  form.managedAgentCatalog.value = createManagedAgentCatalog({
    agentIds: ["codex"],
    defaultAgentId: "codex",
  })
  form.draftProjectPath.value = "/repo"
  form.launchPreview.value = createLaunchPreview()
  form.draftPromptBlocks.value = [{ type: "text", text: "Run the smoke test." }]

  expect(form.sessionInput.value).toMatchObject({
    launchLeaseId: "lease_1",
    localCheckout: undefined,
  })

  form.draftBaseBranchName.value = "feature/testing"

  expect(form.sessionInput.value).toMatchObject({
    launchLeaseId: undefined,
    localCheckout: {
      branchName: "feature/testing",
    },
  })
})

test("SessionLaunchFormState forces bare repositories into worktree launches", () => {
  preferredLaunchAgentId.value = null
  const form = new SessionLaunchFormState()

  form.managedAgentCatalog.value = createManagedAgentCatalog({
    agentIds: ["codex"],
    defaultAgentId: "codex",
  })
  form.draftProjectPath.value = "/bare-repo"
  form.launchPreview.value = createLaunchPreview({
    repoRoot: "/bare-repo",
    bare: true,
    currentBranch: null,
  })
  form.draftPromptBlocks.value = [{ type: "text", text: "Prepare a change." }]
  form.setLaunchLocation("local")

  expect(form.draftLocation.value).toBe("worktree")
  expect(form.sessionInput.value).toMatchObject({
    cwd: "/bare-repo",
    worktree: {
      enabled: true,
    },
  })
})

test("SessionLaunchFormState falls back to the first valid model when managed-agent models change", () => {
  preferredLaunchAgentId.value = null
  const form = new SessionLaunchFormState()

  form.managedAgentCatalog.value = createManagedAgentCatalog({
    agentIds: ["codex"],
    defaultAgentId: "codex",
  })
  form.draftProjectPath.value = "/repo"
  form.launchPreview.value = createLaunchPreview()
  form.draftModelId.value = "opus"
  form.launchPreview.value = createLaunchPreview({
    configOptions: [
      createFixtureModelConfigOption({
        currentValue: "removed-model",
        models: [
          {
            modelId: "haiku",
            name: "Haiku",
          },
        ],
      }),
    ],
  })

  expect(form.draftModelId.value).toBe("haiku")
})

test("filterSlashCommandSuggestions preserves the default cap and fuzzy matches command text", () => {
  const suggestions = createLaunchPreview().slashCommands

  expect(filterSlashCommandSuggestions(suggestions, "")).toEqual(suggestions)
  expect(filterSlashCommandSuggestions(suggestions, "rvw")).toEqual([suggestions[1]])
})
