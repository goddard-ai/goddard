import { rmSync } from "node:fs"
import { getDatabasePath } from "@goddard-ai/paths/node"
import type { DaemonSession } from "@goddard-ai/session/schema"

import { openComposedDaemonStore, type ComposedDaemonStore } from "../plugins.ts"

const MOCK_NOW = 1_750_000_000_000
const MOCK_PROFILE = "mock"

type SeedMockDataOptions = {
  reset?: boolean
}

type SessionSeed = {
  id: DaemonSession["id"]
  acpSessionId: string
  title: string
  status: DaemonSession["status"]
  stopReason: DaemonSession["stopReason"]
  blockedReason: string | null
  errorMessage: string | null
  lastAgentMessage: string | null
  repository: string | null
  prNumber: number | null
  token: string | null
  permissions: DaemonSession["permissions"]
  initiative: string | null
  inboxScope: string | null
  timestamp: number
}

const cwd = "/mock/goddard-ai"

const sessionSeeds: SessionSeed[] = [
  {
    id: "ses_mock_review_boundary",
    acpSessionId: "acp_mock_review_boundary",
    title: "Review app data loading boundary",
    status: "done",
    stopReason: "end_turn",
    blockedReason: null,
    errorMessage: null,
    lastAgentMessage: "I verified the data boundary and left follow-up notes.",
    repository: "goddard-ai/goddard-ai",
    prNumber: 123,
    token: null,
    permissions: null,
    initiative: "Verify the app reads shared daemon state without local mock data.",
    inboxScope: "goddard-ai/goddard-ai#123",
    timestamp: MOCK_NOW - 1_000,
  },
  {
    id: "ses_mock_blocked_config",
    acpSessionId: "acp_mock_blocked_config",
    title: "Resolve config reload ambiguity",
    status: "blocked",
    stopReason: null,
    blockedReason: "Waiting for confirmation on repository-local config precedence.",
    errorMessage: null,
    lastAgentMessage: "The implementation is blocked until config precedence is confirmed.",
    repository: "goddard-ai/goddard-ai",
    prNumber: 124,
    token: null,
    permissions: null,
    initiative: "Clarify hot reload behavior for repository-scoped configuration.",
    inboxScope: "goddard-ai/goddard-ai#124",
    timestamp: MOCK_NOW - 2_000,
  },
  {
    id: "ses_mock_error_worktree",
    acpSessionId: "acp_mock_error_worktree",
    title: "Diagnose worktree preparation failure",
    status: "error",
    stopReason: null,
    blockedReason: null,
    errorMessage: "Mock fixture: package manager bootstrap failed.",
    lastAgentMessage: null,
    repository: "goddard-ai/goddard-ai",
    prNumber: 125,
    token: null,
    permissions: null,
    initiative: "Inspect failed worktree bootstrap diagnostics.",
    inboxScope: "goddard-ai/goddard-ai#125",
    timestamp: MOCK_NOW - 3_000,
  },
]

/** Seeds deterministic local-only daemon records for the isolated mock data profile. */
export async function seedMockData(options: SeedMockDataOptions = {}) {
  const previousProfile = process.env.GODDARD_DATA_PROFILE
  process.env.GODDARD_DATA_PROFILE = MOCK_PROFILE

  try {
    const databasePath = getDatabasePath()
    if (options.reset) {
      removeDatabaseArtifacts(databasePath)
    }

    const store = openComposedDaemonStore()
    try {
      seedStore(store)
    } finally {
      store.close()
    }

    return { databasePath }
  } finally {
    restoreDataProfile(previousProfile)
  }
}

function seedStore(store: ComposedDaemonStore) {
  seedSessions(store)
  seedPullRequests(store)
  seedInbox(store)
}

function seedSessions(store: ComposedDaemonStore) {
  for (const seed of sessionSeeds) {
    withFixtureTime(seed.timestamp, () => {
      store.sessions.put(seed.id, {
        acpSessionId: seed.acpSessionId,
        status: seed.status,
        stopReason: seed.stopReason,
        agent: "mock-agent",
        agentName: "Mock Agent",
        cwd,
        title: seed.title,
        titleState: "generated",
        mcpServers: [],
        connectionMode: "history",
        supportsLoadSession: false,
        activeDaemonSession: false,
        completedHidden: false,
        errorMessage: seed.errorMessage,
        blockedReason: seed.blockedReason,
        initiative: seed.initiative,
        inboxScope: seed.inboxScope,
        lastAgentMessage: seed.lastAgentMessage,
        repository: seed.repository,
        prNumber: seed.prNumber,
        token: seed.token,
        permissions: seed.permissions,
        metadata: {
          mock: true,
        },
        models: null,
        configOptions: [],
        availableCommands: [
          {
            name: "summarize",
            description: "Summarize the current session",
            input: { hint: "Focus area" },
          },
        ],
        contextUsage: {
          size: 200_000,
          used: seed.status === "blocked" ? 84_000 : 42_000,
        },
      })
    })
  }

  withFixtureTime(MOCK_NOW - 1_000, () => {
    store.sessionTurns.put("trn_mock_review_boundary_1", {
      sessionId: "ses_mock_review_boundary",
      turnId: "turn_mock_review_boundary_1",
      sequence: 1,
      promptRequestId: "prompt_mock_review_boundary_1",
      startedAt: new Date(MOCK_NOW - 90_000).toISOString(),
      completedAt: new Date(MOCK_NOW - 60_000).toISOString(),
      completionKind: "result",
      stopReason: "end_turn",
      inboxScope: "goddard-ai/goddard-ai#123",
      inboxHeadline: "Data boundary review complete",
      messages: [
        {
          jsonrpc: "2.0",
          method: "session/prompt",
          params: {
            prompt: [{ type: "text", text: "Review the app data loading boundary." }],
          },
        },
        {
          jsonrpc: "2.0",
          method: "session/update",
          params: {
            update: {
              sessionUpdate: "agent_message_chunk",
              content: {
                type: "text",
                text: "The app should consume daemon-backed state through the SDK.",
              },
            },
          },
        },
      ],
    })
  })

  withFixtureTime(MOCK_NOW - 2_000, () => {
    store.sessionTurns.put("trn_mock_blocked_config_1", {
      sessionId: "ses_mock_blocked_config",
      turnId: "turn_mock_blocked_config_1",
      sequence: 1,
      promptRequestId: "prompt_mock_blocked_config_1",
      startedAt: new Date(MOCK_NOW - 180_000).toISOString(),
      completedAt: null,
      completionKind: null,
      stopReason: null,
      inboxScope: "goddard-ai/goddard-ai#124",
      inboxHeadline: "Config precedence needs review",
      messages: [
        {
          jsonrpc: "2.0",
          method: "session/update",
          params: {
            update: {
              sessionUpdate: "agent_message_chunk",
              content: {
                type: "text",
                text: "I need the intended config precedence before changing reload behavior.",
              },
            },
          },
        },
      ],
    })
  })
}

function seedPullRequests(store: ComposedDaemonStore) {
  for (const prNumber of [123, 124, 125]) {
    withFixtureTime(MOCK_NOW - prNumber, () => {
      store.pullRequests.put(`pr_mock_${prNumber}` as `pr_${string}`, {
        host: "github",
        owner: "goddard-ai",
        repo: "goddard-ai",
        prNumber,
        cwd,
      })
    })
  }
}

function seedInbox(store: ComposedDaemonStore) {
  const items: Array<Parameters<typeof store.inboxItems.put>[1] & { id: `inb_${string}` }> = [
    {
      id: "inb_mock_review_boundary",
      entityId: "ses_mock_review_boundary",
      reason: "session.turn_ended",
      status: "unread",
      priority: "normal",
      updatedAt: MOCK_NOW - 1_000,
      readAt: null,
      scope: "goddard-ai/goddard-ai#123",
      headline: "Data boundary review complete",
      turnId: "turn_mock_review_boundary_1",
    },
    {
      id: "inb_mock_blocked_config",
      entityId: "ses_mock_blocked_config",
      reason: "session.blocked",
      status: "saved",
      priority: "normal",
      updatedAt: MOCK_NOW - 2_000,
      readAt: MOCK_NOW - 1_500,
      scope: "goddard-ai/goddard-ai#124",
      headline: "Config precedence needs review",
      turnId: "turn_mock_blocked_config_1",
    },
    {
      id: "inb_mock_error_worktree",
      entityId: "ses_mock_error_worktree",
      reason: "session.turn_ended",
      status: "archived",
      priority: "low",
      updatedAt: MOCK_NOW - 3_000,
      readAt: MOCK_NOW - 2_500,
      scope: "goddard-ai/goddard-ai#125",
      headline: "Worktree bootstrap failed",
      turnId: null,
    },
    {
      id: "inb_mock_pull_request",
      entityId: "pr_mock_123",
      reason: "pull_request.updated",
      status: "read",
      priority: "normal",
      updatedAt: MOCK_NOW - 4_000,
      readAt: MOCK_NOW - 3_500,
      scope: "goddard-ai/goddard-ai#123",
      headline: "Reviewer requested a narrower data-boundary check",
      turnId: "turn_mock_review_boundary_1",
    },
  ]

  for (const item of items) {
    const { id, ...input } = item
    store.inboxItems.put(id, input)
  }
}

function removeDatabaseArtifacts(filename: string) {
  for (const suffix of ["", "-shm", "-wal"]) {
    rmSync(`${filename}${suffix}`, { force: true })
  }
}

function withFixtureTime<T>(timestamp: number, callback: () => T): T {
  const originalNow = Date.now
  Date.now = () => timestamp
  try {
    return callback()
  } finally {
    Date.now = originalNow
  }
}

function restoreDataProfile(value: string | undefined) {
  if (value === undefined) {
    delete process.env.GODDARD_DATA_PROFILE
    return
  }

  process.env.GODDARD_DATA_PROFILE = value
}
