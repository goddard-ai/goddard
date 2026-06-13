import { createCriticalAppStatesScenario } from "@goddard-ai/fixtures"

const criticalStates = createCriticalAppStatesScenario()

export const blockedSession = criticalStates.sessions.blockedSession
export const activeSession = criticalStates.sessions.activeSession
export const errorSession = criticalStates.sessions.errorSession
export const completedSession = criticalStates.sessions.completedSession

export const criticalSessionsResponse = criticalStates.sessions.response
export const inboxAttentionResponse = criticalStates.inbox.response
export const reviewPullRequestResponse = criticalStates.pullRequest.response
export const blockedSessionResponse = criticalStates.blockedSession.sessionResponse
export const blockedSessionHistoryResponse = criticalStates.blockedSession.historyResponse
export const blockedSessionWorktreeResponse = criticalStates.blockedSession.worktreeResponse
export const blockedSessionChangesResponse = criticalStates.blockedSession.changesResponse
