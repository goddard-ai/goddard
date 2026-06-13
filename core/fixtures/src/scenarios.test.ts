import { expect, test } from "bun:test"

import {
  createBlockedSessionScenario,
  createCriticalAppStatesScenario,
  createSessionTriageQueueScenario,
  criticalAppStateIds,
} from "./index.ts"

test("session triage scenario exports stable named session ids", () => {
  const scenario = createSessionTriageQueueScenario()

  expect(scenario.blockedSession.id).toBe(criticalAppStateIds.sessions.blocked)
  expect(scenario.activeSession.id).toBe(criticalAppStateIds.sessions.active)
  expect(scenario.errorSession.id).toBe(criticalAppStateIds.sessions.error)
  expect(scenario.completedSession.id).toBe(criticalAppStateIds.sessions.completed)
  expect(scenario.response.sessions.map((session) => session.id)).toEqual([
    criticalAppStateIds.sessions.blocked,
    criticalAppStateIds.sessions.active,
    criticalAppStateIds.sessions.error,
    criticalAppStateIds.sessions.completed,
  ])
})

test("inbox attention scenario links rows to session and pull request entities", () => {
  const scenario = createCriticalAppStatesScenario()

  expect(scenario.inbox.response.items.map((item) => item.entityId)).toEqual([
    scenario.sessions.blockedSession.id,
    scenario.pullRequest.pullRequest.id,
    scenario.sessions.activeSession.id,
  ])
})

test("blocked session scenario keeps identity consistent across responses", () => {
  const scenario = createBlockedSessionScenario()

  expect(scenario.sessionResponse.session.id).toBe(scenario.session.id)
  expect(scenario.historyResponse.id).toBe(scenario.session.id)
  expect(scenario.worktreeResponse.id).toBe(scenario.session.id)
  expect(scenario.changesResponse.id).toBe(scenario.session.id)
  expect(scenario.historyResponse.turns[0]?.turnId).toBe(criticalAppStateIds.turns.blocked)
})
