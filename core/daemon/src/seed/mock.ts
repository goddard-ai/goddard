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

type SessionTurnSeed = Parameters<ComposedDaemonStore["sessionTurns"]["put"]>[1] & {
  id: `trn_${string}`
  timestamp: number
}

type PullRequestSeed = Parameters<ComposedDaemonStore["pullRequests"]["put"]>[1] & {
  id: `pr_${string}`
  timestamp: number
}

type InboxItemSeed = Parameters<ComposedDaemonStore["inboxItems"]["put"]>[1] & {
  id: `inb_${string}`
}

type MockSeedScenario = {
  id: string
  label: string
  sessions?: SessionSeed[]
  sessionTurns?: SessionTurnSeed[]
  pullRequests?: PullRequestSeed[]
  inboxItems?: InboxItemSeed[]
}

const cwd = "/mock/goddard-ai"

const scenarios: MockSeedScenario[] = [
  {
    id: "review-boundary-complete",
    label: "Completed PR-scoped review with readable session history",
    sessions: [
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
    ],
    sessionTurns: [
      {
        id: "trn_mock_review_boundary_1",
        timestamp: MOCK_NOW - 1_000,
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
      },
    ],
    pullRequests: [
      {
        id: "pr_mock_123",
        timestamp: MOCK_NOW - 123,
        host: "github",
        owner: "goddard-ai",
        repo: "goddard-ai",
        prNumber: 123,
        cwd,
      },
    ],
    inboxItems: [
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
    ],
  },
  {
    id: "config-precedence-blocked",
    label: "Blocked PR-scoped session waiting on a product decision",
    sessions: [
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
    ],
    sessionTurns: [
      {
        id: "trn_mock_blocked_config_1",
        timestamp: MOCK_NOW - 2_000,
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
      },
    ],
    pullRequests: [
      {
        id: "pr_mock_124",
        timestamp: MOCK_NOW - 124,
        host: "github",
        owner: "goddard-ai",
        repo: "goddard-ai",
        prNumber: 124,
        cwd,
      },
    ],
    inboxItems: [
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
    ],
  },
  {
    id: "worktree-bootstrap-error",
    label: "Errored PR-scoped session with archived inbox state",
    sessions: [
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
    ],
    pullRequests: [
      {
        id: "pr_mock_125",
        timestamp: MOCK_NOW - 125,
        host: "github",
        owner: "goddard-ai",
        repo: "goddard-ai",
        prNumber: 125,
        cwd,
      },
    ],
    inboxItems: [
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
    ],
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
  for (const scenario of scenarios) {
    seedScenario(store, scenario)
  }
}

function seedScenario(store: ComposedDaemonStore, scenario: MockSeedScenario) {
  seedSessions(store, scenario.sessions ?? [])
  seedSessionTurns(store, scenario.sessionTurns ?? [])
  seedPullRequests(store, scenario.pullRequests ?? [])
  seedInbox(store, scenario.inboxItems ?? [])
}

function seedSessions(store: ComposedDaemonStore, seeds: SessionSeed[]) {
  for (const seed of seeds) {
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
          scenario: seedScenarioLabel(seed.id),
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
}

function seedSessionTurns(store: ComposedDaemonStore, seeds: SessionTurnSeed[]) {
  for (const seed of seeds) {
    const { id, timestamp, ...input } = seed
    withFixtureTime(timestamp, () => {
      store.sessionTurns.put(id, input)
    })
  }
}

function seedPullRequests(store: ComposedDaemonStore, seeds: PullRequestSeed[]) {
  for (const seed of seeds) {
    const { id, timestamp, ...input } = seed
    withFixtureTime(timestamp, () => {
      store.pullRequests.put(id, input)
    })
  }
}

function seedInbox(store: ComposedDaemonStore, seeds: InboxItemSeed[]) {
  for (const seed of seeds) {
    const { id, ...input } = seed
    store.inboxItems.put(id, input)
  }
}

function seedScenarioLabel(sessionId: DaemonSession["id"]) {
  return scenarios.find((scenario) =>
    scenario.sessions?.some((session) => session.id === sessionId),
  )?.label
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
