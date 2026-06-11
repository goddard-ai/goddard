import type { DaemonSession } from "@goddard-ai/sdk"
import { expect, mock, test, vi } from "bun:test"
import { render } from "preact"
import { act } from "preact/test-utils"

import { ListRow } from "./list-row.tsrx"
import { SessionsList } from "./list.tsrx"
import { SessionsPageMutations } from "./mutations.ts"

function Icon(props: Record<string, unknown>) {
  return <svg {...props} />
}

mock.module("lucide-react", () => ({
  AlertCircle: Icon,
  CheckCircle2: Icon,
  CircleDot: Icon,
  FileDiff: Icon,
  LoaderCircle: Icon,
  PauseCircle: Icon,
  XCircle: Icon,
}))

function createSession(overrides: Partial<DaemonSession> = {}): DaemonSession {
  return {
    id: "ses_session_1",
    acpSessionId: "acp-session-1",
    status: "active",
    stopReason: null,
    agent: "codex",
    agentName: "Codex",
    cwd: "/repo",
    mcpServers: [],
    connectionMode: "live",
    supportsLoadSession: true,
    activeDaemonSession: true,
    completedHidden: false,
    token: null,
    permissions: null,
    title: "Stabilize app sessions",
    titleState: "generated",
    repository: "goddard-ai",
    prNumber: null,
    metadata: null,
    createdAt: 1_800_000_000_000,
    updatedAt: Date.now(),
    errorMessage: null,
    blockedReason: null,
    initiative: null,
    inboxScope: null,
    lastAgentMessage: null,
    models: null,
    configOptions: [],
    availableCommands: [],
    contextUsage: null,
    ...overrides,
  }
}

function renderListRow(input: {
  openSession?: (sessionId: DaemonSession["id"]) => void
  openSessionChanges?: (sessionId: DaemonSession["id"]) => void
  session?: DaemonSession
}) {
  const container = document.createElement("div")
  const openSession = input.openSession ?? vi.fn()
  const openSessionChanges = input.openSessionChanges ?? vi.fn()
  const session = input.session ?? createSession()

  document.body.append(container)
  render(
    <SessionsPageMutations
      mutations={{
        openSession,
        openSessionChanges,
      }}
    >
      <ListRow session={session} />
    </SessionsPageMutations>,
    container,
  )

  return {
    container,
    openSession,
    openSessionChanges,
    row: container.querySelector(".session-list-row") as HTMLElement,
  }
}

test("ListRow opens a session by pointer and keyboard activation", async () => {
  const openSession = vi.fn()
  const harness = renderListRow({ openSession })

  await act(async () => {
    harness.row.click()
    harness.row.dispatchEvent(
      new KeyboardEvent("keydown", {
        bubbles: true,
        cancelable: true,
        key: "Enter",
      }),
    )
  })

  expect(openSession).toHaveBeenCalledTimes(2)
  expect(openSession).toHaveBeenNthCalledWith(1, "ses_session_1")
  expect(openSession).toHaveBeenNthCalledWith(2, "ses_session_1")

  render(null, harness.container)
  harness.container.remove()
})

test("ListRow opens session changes without also opening the session", async () => {
  const openSession = vi.fn()
  const openSessionChanges = vi.fn()
  const harness = renderListRow({ openSession, openSessionChanges })
  const changesButton = harness.container.querySelector(
    "button[aria-label='View changes for Stabilize app sessions']",
  ) as HTMLButtonElement

  await act(async () => {
    changesButton.click()
  })

  expect(openSession).not.toHaveBeenCalled()
  expect(openSessionChanges).toHaveBeenCalledWith("ses_session_1")

  render(null, harness.container)
  harness.container.remove()
})

test("SessionsList renders loading, error, empty, and row states from observable props", () => {
  const container = document.createElement("div")
  document.body.append(container)

  render(<SessionsList listStatus="loading" searchQuery="" sessions={[]} />, container)
  expect(container.textContent).toContain("Loading sessions")

  render(
    <SessionsList
      errorMessage="Daemon unavailable"
      listStatus="error"
      searchQuery=""
      sessions={[]}
    />,
    container,
  )
  expect(container.textContent).toContain("Couldn't load sessions")
  expect(container.textContent).toContain("Daemon unavailable")

  render(<SessionsList searchQuery="missing" sessions={[]} />, container)
  expect(container.textContent).toContain("No matching sessions")

  render(
    <SessionsPageMutations
      mutations={{
        openSession() {},
        openSessionChanges() {},
      }}
    >
      <SessionsList searchQuery="" sessions={[createSession()]} />
    </SessionsPageMutations>,
    container,
  )
  expect(container.querySelector(".session-list-row")?.textContent).toContain(
    "Stabilize app sessions",
  )

  render(null, container)
  container.remove()
})
