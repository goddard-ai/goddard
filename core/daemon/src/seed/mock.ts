import { rmSync } from "node:fs"
import { getDatabasePath } from "@goddard-ai/paths/node"
import type { DaemonSession } from "@goddard-ai/session/schema"

import { openComposedDaemonStore, type ComposedDaemonStore } from "../plugins.ts"

const MOCK_NOW = 1_750_000_000_000
const MOCK_PROFILE = "mock"

type SeedMockDataOptions = {
  reset?: boolean
}

type SessionSeed = Parameters<ComposedDaemonStore["sessions"]["put"]>[1] & {
  id: DaemonSession["id"]
  timestamp: number
}

type SessionSeedOverrides = Pick<SessionSeed, "id" | "title" | "timestamp"> &
  Partial<Omit<SessionSeed, "id" | "title" | "timestamp">>

type SessionTurnSeed = Parameters<ComposedDaemonStore["sessionTurns"]["put"]>[1] & {
  id: `trn_${string}`
  timestamp: number
}

type SessionTurnSeedOverrides = Pick<
  SessionTurnSeed,
  "id" | "messages" | "sequence" | "sessionId" | "timestamp" | "turnId"
> &
  Partial<
    Omit<SessionTurnSeed, "id" | "messages" | "sequence" | "sessionId" | "timestamp" | "turnId">
  >

type PullRequestSeed = Parameters<ComposedDaemonStore["pullRequests"]["put"]>[1] & {
  id: `pr_${string}`
  timestamp: number
}

type PullRequestSeedOverrides = Pick<PullRequestSeed, "id" | "prNumber" | "timestamp"> &
  Partial<Omit<PullRequestSeed, "id" | "prNumber" | "timestamp">>

type InboxItemSeed = Parameters<ComposedDaemonStore["inboxItems"]["put"]>[1] & {
  id: `inb_${string}`
}

type InboxItemSeedOverrides = Pick<InboxItemSeed, "entityId" | "headline" | "id"> &
  Partial<Omit<InboxItemSeed, "entityId" | "headline" | "id">> & {
    timestamp?: number
  }

type MockSeedScenario = {
  id: string
  label: string
  sessions?: SessionSeedOverrides[]
  sessionTurns?: SessionTurnSeedOverrides[]
  pullRequests?: PullRequestSeedOverrides[]
  inboxItems?: InboxItemSeedOverrides[]
}

const cwd = "/mock/goddard-ai"

function createSessionSeed(overrides: SessionSeedOverrides, scenarioLabel: string): SessionSeed {
  const status = overrides.status ?? "done"

  return {
    acpSessionId: overrides.acpSessionId ?? overrides.id.replace(/^ses_/, "acp_"),
    status,
    stopReason: overrides.stopReason ?? defaultStopReason(status),
    agent: "mock-agent",
    agentName: "Mock Agent",
    cwd,
    titleState: "generated",
    lastSessionActivityAt: overrides.lastSessionActivityAt ?? overrides.timestamp,
    mcpServers: [],
    connectionMode: "history",
    supportsLoadSession: false,
    activeDaemonSession: false,
    completedHidden: false,
    errorMessage: null,
    blockedReason: null,
    initiative: null,
    inboxScope: null,
    lastAgentMessage: null,
    repository: "goddard-ai/goddard-ai",
    prNumber: null,
    token: null,
    permissions: null,
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
      used: status === "blocked" ? 84_000 : 42_000,
    },
    ...overrides,
    metadata: {
      mock: true,
      scenario: scenarioLabel,
      ...overrides.metadata,
    },
  }
}

function createTurnSeed(overrides: SessionTurnSeedOverrides): SessionTurnSeed {
  return {
    promptRequestId: overrides.turnId.replace(/^turn_/, "prompt_"),
    startedAt: new Date(overrides.timestamp - 30_000).toISOString(),
    completedAt: new Date(overrides.timestamp).toISOString(),
    completionKind: "result",
    stopReason: "end_turn",
    inboxScope: null,
    inboxHeadline: null,
    ...overrides,
  }
}

function createPullRequestSeed(overrides: PullRequestSeedOverrides): PullRequestSeed {
  return {
    host: "github",
    owner: "goddard-ai",
    repo: "goddard-ai",
    cwd,
    ...overrides,
  }
}

function createInboxItemSeed(overrides: InboxItemSeedOverrides): InboxItemSeed {
  const updatedAt = overrides.timestamp ?? overrides.updatedAt ?? MOCK_NOW
  const { timestamp: _timestamp, ...input } = overrides

  return {
    reason: "session.turn_ended",
    status: "unread",
    priority: "normal",
    updatedAt,
    readAt: null,
    scope: null,
    turnId: null,
    ...input,
  }
}

function defaultStopReason(status: DaemonSession["status"]): DaemonSession["stopReason"] {
  if (status === "cancelled") {
    return "cancelled"
  }

  if (status === "archived" || status === "done") {
    return "end_turn"
  }

  return null
}

const scenarios: MockSeedScenario[] = [
  {
    id: "review-boundary-complete",
    label: "Completed PR-scoped review with readable session history",
    sessions: [
      {
        id: "ses_mock_review_boundary",
        title: "Review app data loading boundary",
        status: "done",
        lastAgentMessage: "I verified the data boundary and left follow-up notes.",
        prNumber: 123,
        initiative: "Verify the app reads shared daemon state without local mock data.",
        inboxScope: "goddard-ai/goddard-ai#123",
        availableCommands: [
          {
            name: "plan",
            description: "Create or revise the plan",
            input: { hint: "What should change?" },
          },
          {
            name: "summarize",
            description: "Summarize the current session",
            input: { hint: "Focus area" },
          },
        ],
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
        startedAt: new Date(MOCK_NOW - 90_000).toISOString(),
        completedAt: new Date(MOCK_NOW - 60_000).toISOString(),
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
      {
        id: "trn_mock_review_boundary_2",
        timestamp: MOCK_NOW - 500,
        sessionId: "ses_mock_review_boundary",
        turnId: "turn_mock_review_boundary_2",
        sequence: 2,
        startedAt: new Date(MOCK_NOW - 55_000).toISOString(),
        completedAt: new Date(MOCK_NOW - 20_000).toISOString(),
        inboxScope: "goddard-ai/goddard-ai#123",
        inboxHeadline: "Follow-up notes added",
        messages: [
          {
            jsonrpc: "2.0",
            method: "session/prompt",
            params: {
              prompt: [{ type: "text", text: "Add a concise follow-up summary." }],
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
                  text: "Follow-up notes now call out SDK parity and daemon-owned state.",
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
        prNumber: 123,
      },
    ],
    inboxItems: [
      {
        id: "inb_mock_review_boundary",
        entityId: "ses_mock_review_boundary",
        reason: "session.turn_ended",
        status: "unread",
        updatedAt: MOCK_NOW - 1_000,
        scope: "goddard-ai/goddard-ai#123",
        headline: "Data boundary review complete",
        turnId: "turn_mock_review_boundary_1",
      },
      {
        id: "inb_mock_pull_request",
        entityId: "pr_mock_123",
        reason: "pull_request.updated",
        status: "read",
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
        title: "Resolve config reload ambiguity",
        status: "blocked",
        stopReason: null,
        blockedReason: "Waiting for confirmation on repository-local config precedence.",
        lastAgentMessage: "The implementation is blocked until config precedence is confirmed.",
        prNumber: 124,
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
        prNumber: 124,
      },
    ],
    inboxItems: [
      {
        id: "inb_mock_blocked_config",
        entityId: "ses_mock_blocked_config",
        reason: "session.blocked",
        status: "saved",
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
        title: "Diagnose worktree preparation failure",
        status: "error",
        stopReason: null,
        errorMessage: "Mock fixture: package manager bootstrap failed.",
        lastAgentMessage: null,
        prNumber: 125,
        initiative: "Inspect failed worktree bootstrap diagnostics.",
        inboxScope: "goddard-ai/goddard-ai#125",
        timestamp: MOCK_NOW - 3_000,
      },
    ],
    pullRequests: [
      {
        id: "pr_mock_125",
        timestamp: MOCK_NOW - 125,
        prNumber: 125,
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
      },
    ],
  },
  {
    id: "local-idle-planning",
    label: "Idle local-only session with no repository scope",
    sessions: [
      {
        id: "ses_mock_local_idle",
        title: "Explore local onboarding cleanup",
        status: "idle",
        stopReason: null,
        lastAgentMessage: "I found a small documentation cleanup path for local onboarding.",
        repository: null,
        initiative: "Collect local-only onboarding cleanup ideas.",
        inboxScope: "local workspace",
        timestamp: MOCK_NOW - 4_000,
      },
    ],
    sessionTurns: [
      {
        id: "trn_mock_local_idle_1",
        timestamp: MOCK_NOW - 4_000,
        sessionId: "ses_mock_local_idle",
        turnId: "turn_mock_local_idle_1",
        sequence: 1,
        startedAt: new Date(MOCK_NOW - 240_000).toISOString(),
        completedAt: new Date(MOCK_NOW - 210_000).toISOString(),
        inboxScope: "local workspace",
        inboxHeadline: "Onboarding cleanup ideas collected",
        messages: [
          {
            jsonrpc: "2.0",
            method: "session/update",
            params: {
              update: {
                sessionUpdate: "agent_message_chunk",
                content: {
                  type: "text",
                  text: "The local setup notes can be shortened without changing behavior.",
                },
              },
            },
          },
        ],
      },
    ],
    inboxItems: [
      {
        id: "inb_mock_local_idle",
        entityId: "ses_mock_local_idle",
        reason: "session.turn_ended",
        status: "read",
        priority: "low",
        updatedAt: MOCK_NOW - 4_000,
        readAt: MOCK_NOW - 3_800,
        scope: "local workspace",
        headline: "Onboarding cleanup ideas collected",
        turnId: "turn_mock_local_idle_1",
      },
    ],
  },
  {
    id: "cancelled-dependency-audit",
    label: "Cancelled local-only session with replied inbox state",
    sessions: [
      {
        id: "ses_mock_cancelled_audit",
        title: "Cancel outdated dependency audit",
        status: "cancelled",
        stopReason: "cancelled",
        lastAgentMessage: "The dependency audit was cancelled before making changes.",
        repository: null,
        initiative: "Audit dependency drift without changing package versions.",
        inboxScope: "local workspace",
        timestamp: MOCK_NOW - 5_000,
      },
    ],
    sessionTurns: [
      {
        id: "trn_mock_cancelled_audit_1",
        timestamp: MOCK_NOW - 5_000,
        sessionId: "ses_mock_cancelled_audit",
        turnId: "turn_mock_cancelled_audit_1",
        sequence: 1,
        startedAt: new Date(MOCK_NOW - 320_000).toISOString(),
        completedAt: new Date(MOCK_NOW - 300_000).toISOString(),
        stopReason: "cancelled",
        inboxScope: "local workspace",
        inboxHeadline: "Audit cancelled before changes",
        messages: [
          {
            jsonrpc: "2.0",
            method: "session/update",
            params: {
              update: {
                sessionUpdate: "agent_message_chunk",
                content: {
                  type: "text",
                  text: "Cancellation observed before any dependency files were edited.",
                },
              },
            },
          },
        ],
      },
    ],
    inboxItems: [
      {
        id: "inb_mock_cancelled_audit",
        entityId: "ses_mock_cancelled_audit",
        reason: "session.turn_ended",
        status: "replied",
        updatedAt: MOCK_NOW - 5_000,
        readAt: MOCK_NOW - 4_800,
        scope: "local workspace",
        headline: "Audit cancelled before changes",
        turnId: "turn_mock_cancelled_audit_1",
      },
    ],
  },
  {
    id: "archived-release-notes",
    label: "Archived history-only session with completed inbox state",
    sessions: [
      {
        id: "ses_mock_archived_release_notes",
        title: "Archive release notes cleanup",
        status: "archived",
        lastAgentMessage: "Release note cleanup was archived after completion.",
        initiative: "Clean up release note wording for local review.",
        inboxScope: "goddard-ai/goddard-ai",
        timestamp: MOCK_NOW - 6_000,
      },
    ],
    sessionTurns: [
      {
        id: "trn_mock_archived_release_notes_1",
        timestamp: MOCK_NOW - 6_000,
        sessionId: "ses_mock_archived_release_notes",
        turnId: "turn_mock_archived_release_notes_1",
        sequence: 1,
        startedAt: new Date(MOCK_NOW - 420_000).toISOString(),
        completedAt: new Date(MOCK_NOW - 390_000).toISOString(),
        inboxScope: "goddard-ai/goddard-ai",
        inboxHeadline: "Release notes cleanup archived",
        messages: [
          {
            jsonrpc: "2.0",
            method: "session/update",
            params: {
              update: {
                sessionUpdate: "agent_message_chunk",
                content: {
                  type: "text",
                  text: "The archived cleanup kept the release note changes small.",
                },
              },
            },
          },
        ],
      },
    ],
    inboxItems: [
      {
        id: "inb_mock_archived_release_notes",
        entityId: "ses_mock_archived_release_notes",
        reason: "session.turn_ended",
        status: "completed",
        priority: "low",
        updatedAt: MOCK_NOW - 6_000,
        readAt: MOCK_NOW - 5_800,
        scope: "goddard-ai/goddard-ai",
        headline: "Release notes cleanup archived",
        turnId: "turn_mock_archived_release_notes_1",
      },
    ],
  },
  {
    id: "long-content-layout",
    label: "Long title and headline stress case for compact UI surfaces",
    sessions: [
      {
        id: "ses_mock_long_content",
        title:
          "Review the exceptionally long workspace navigation label and ensure it wraps cleanly in compact session lists",
        status: "done",
        lastAgentMessage:
          "This deliberately long message checks whether the session list, inbox preview, and detail header preserve readable wrapping without overlapping nearby controls.",
        initiative: "Stress-test long copy in session and inbox surfaces.",
        inboxScope: "goddard-ai/goddard-ai",
        timestamp: MOCK_NOW - 7_000,
      },
    ],
    inboxItems: [
      {
        id: "inb_mock_long_content",
        entityId: "ses_mock_long_content",
        reason: "session.turn_ended",
        status: "saved",
        updatedAt: MOCK_NOW - 7_000,
        readAt: MOCK_NOW - 6_800,
        scope: "goddard-ai/goddard-ai with a deliberately long scope label",
        headline:
          "Long inbox headline used to verify wrapping behavior in dense lists and narrow panes",
      },
    ],
  },
  {
    id: "context-near-limit",
    label: "Session near context limit with model and config option state",
    sessions: [
      {
        id: "ses_mock_context_limit",
        title: "Summarize near-limit planning context",
        status: "done",
        stopReason: "max_tokens",
        lastAgentMessage: "Context usage is near the configured model limit.",
        initiative: "Summarize a large planning context before continuing.",
        inboxScope: "goddard-ai/goddard-ai",
        models: {
          currentModelId: "gpt-5.4",
          availableModels: [
            {
              modelId: "gpt-5.4",
              name: "GPT-5.4",
              description: "Balanced frontier model",
            },
            {
              modelId: "gpt-5.4-mini",
              name: "GPT-5.4 Mini",
              description: "Faster lower-latency variant",
            },
          ],
        },
        configOptions: [
          {
            id: "thinking",
            type: "select",
            name: "Thinking level",
            category: "thought_level",
            description: "Select how much reasoning budget to use.",
            currentValue: "high",
            options: [
              { value: "low", name: "Low", description: "Keep reasoning light." },
              { value: "high", name: "High", description: "Spend more reasoning effort." },
            ],
          },
        ],
        contextUsage: {
          size: 200_000,
          used: 194_000,
        },
        timestamp: MOCK_NOW - 8_000,
      },
    ],
    sessionTurns: [
      {
        id: "trn_mock_context_limit_1",
        timestamp: MOCK_NOW - 8_000,
        sessionId: "ses_mock_context_limit",
        turnId: "turn_mock_context_limit_1",
        sequence: 1,
        startedAt: new Date(MOCK_NOW - 520_000).toISOString(),
        completedAt: new Date(MOCK_NOW - 500_000).toISOString(),
        stopReason: "max_tokens",
        inboxScope: "goddard-ai/goddard-ai",
        inboxHeadline: "Context summary reached token limit",
        messages: [
          {
            jsonrpc: "2.0",
            method: "session/update",
            params: {
              update: {
                sessionUpdate: "agent_message_chunk",
                content: {
                  type: "text",
                  text: "The summary is ready, but the session is close to the context limit.",
                },
              },
            },
          },
        ],
      },
    ],
  },
  {
    id: "docs-repo-pr-created",
    label: "Created pull request record in a second repository",
    pullRequests: [
      {
        id: "pr_mock_docs_7",
        timestamp: MOCK_NOW - 9_000,
        repo: "docs",
        prNumber: 7,
        cwd: "/mock/goddard-docs",
      },
    ],
    inboxItems: [
      {
        id: "inb_mock_docs_pr_created",
        entityId: "pr_mock_docs_7",
        reason: "pull_request.created",
        status: "unread",
        updatedAt: MOCK_NOW - 9_000,
        scope: "goddard-ai/docs#7",
        headline: "Documentation navigation PR created",
      },
    ],
  },
  {
    id: "tools-repo-pr-updated",
    label: "Updated pull request record in a third repository",
    pullRequests: [
      {
        id: "pr_mock_tools_42",
        timestamp: MOCK_NOW - 10_000,
        owner: "acme",
        repo: "developer-tools",
        prNumber: 42,
        cwd: "/mock/developer-tools",
      },
    ],
    inboxItems: [
      {
        id: "inb_mock_tools_pr_updated",
        entityId: "pr_mock_tools_42",
        reason: "pull_request.updated",
        status: "saved",
        priority: "low",
        updatedAt: MOCK_NOW - 10_000,
        readAt: MOCK_NOW - 9_500,
        scope: "acme/developer-tools#42",
        headline: "Tooling PR received a follow-up review",
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
  seedSessions(
    store,
    (scenario.sessions ?? []).map((session) => createSessionSeed(session, scenario.label)),
  )
  seedSessionTurns(store, (scenario.sessionTurns ?? []).map(createTurnSeed))
  seedPullRequests(store, (scenario.pullRequests ?? []).map(createPullRequestSeed))
  seedInbox(store, (scenario.inboxItems ?? []).map(createInboxItemSeed))
}

function seedSessions(store: ComposedDaemonStore, seeds: SessionSeed[]) {
  for (const seed of seeds) {
    const { id, timestamp, ...input } = seed
    withFixtureTime(timestamp, () => {
      store.sessions.put(id, input)
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
