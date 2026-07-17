import {
  createAcpSessionUpdateMatrixScenario,
  createActiveSessionScenario,
  createBlockedSessionScenario,
  createInboxAttentionQueueScenario,
  createSessionTriageQueueScenario,
} from "@goddard-ai/fixtures"

const sessionTriageQueue = createSessionTriageQueueScenario()
const inboxAttentionQueue = createInboxAttentionQueueScenario({
  activeSession: sessionTriageQueue.activeSession,
  blockedSession: sessionTriageQueue.blockedSession,
})
const activeSessionDetail = createActiveSessionScenario(sessionTriageQueue.activeSession)
const blockedSessionDetail = createBlockedSessionScenario(sessionTriageQueue.blockedSession)
const acpSessionUpdateMatrix = createAcpSessionUpdateMatrixScenario()

export const blockedSession = sessionTriageQueue.blockedSession
export const activeSession = sessionTriageQueue.activeSession
export const errorSession = sessionTriageQueue.errorSession
export const completedSession = sessionTriageQueue.completedSession

export const criticalSessionsResponse = sessionTriageQueue.response
export const inboxAttentionResponse = inboxAttentionQueue.response
export const reviewPullRequestResponse = inboxAttentionQueue.pullRequestResponse
export const activeSessionResponse = activeSessionDetail.sessionResponse
export const activeSessionHistoryResponse = activeSessionDetail.historyResponse
export const activeSessionWorktreeResponse = activeSessionDetail.worktreeResponse
export const blockedSessionResponse = blockedSessionDetail.sessionResponse
export const blockedSessionHistoryResponse = blockedSessionDetail.historyResponse
export const blockedSessionWorktreeResponse = blockedSessionDetail.worktreeResponse
export const blockedSessionChangesResponse = blockedSessionDetail.changesResponse
export const acpSessionUpdateMatrixSession = acpSessionUpdateMatrix.session
export const acpSessionUpdateMatrixSessionResponse = acpSessionUpdateMatrix.sessionResponse
export const acpSessionUpdateMatrixHistoryResponse = acpSessionUpdateMatrix.historyResponse
