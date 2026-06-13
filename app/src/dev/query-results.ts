import {
  createBlockedSessionScenario,
  createInboxAttentionQueueScenario,
  createSessionTriageQueueScenario,
} from "@goddard-ai/fixtures"

const sessionTriageQueue = createSessionTriageQueueScenario()
const inboxAttentionQueue = createInboxAttentionQueueScenario({
  activeSession: sessionTriageQueue.activeSession,
  blockedSession: sessionTriageQueue.blockedSession,
})
const blockedSessionDetail = createBlockedSessionScenario(sessionTriageQueue.blockedSession)

export const blockedSession = sessionTriageQueue.blockedSession
export const activeSession = sessionTriageQueue.activeSession
export const errorSession = sessionTriageQueue.errorSession
export const completedSession = sessionTriageQueue.completedSession

export const criticalSessionsResponse = sessionTriageQueue.response
export const inboxAttentionResponse = inboxAttentionQueue.response
export const reviewPullRequestResponse = inboxAttentionQueue.pullRequestResponse
export const blockedSessionResponse = blockedSessionDetail.sessionResponse
export const blockedSessionHistoryResponse = blockedSessionDetail.historyResponse
export const blockedSessionWorktreeResponse = blockedSessionDetail.worktreeResponse
export const blockedSessionChangesResponse = blockedSessionDetail.changesResponse
