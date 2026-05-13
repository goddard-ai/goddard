import type { DaemonSessionId } from "@goddard-ai/schema/common/params"
import type {
  CancelSessionResponse,
  CompleteSessionResponse,
  CreateSessionRequest,
  DaemonSession,
  GetSessionChangesResponse,
  GetSessionDiagnosticsResponse,
  GetSessionHistoryRequest,
  GetSessionHistoryResponse,
  GetSessionWorkforceResponse,
  InboxHeadline,
  InboxScope,
  ListSessionsRequest,
  ListSessionsResponse,
  SendSessionMessageRequest,
  SessionComposerSuggestionsRequest,
  SessionComposerSuggestionsResponse,
  SessionDraftSuggestionsRequest,
  SessionInboxMetadataInput,
  SessionLaunchPreviewRequest,
  SessionLaunchPreviewResponse,
  SessionSubpackagesRequest,
  SessionSubpackagesResponse,
  SteerSessionRequest,
  SteerSessionResponse,
} from "@goddard-ai/schema/daemon"

import type {
  GetSessionWorktreeResponse,
  MutateSessionReviewSessionResponse,
  SessionWorktreeIdentity,
} from "../schema.ts"

/** Daemon-owned session operations that the session feature plugin composes into handlers. */
export type SessionController = {
  newSession: (params: { request: CreateSessionRequest }) => Promise<DaemonSession>
  listSessions: (params: ListSessionsRequest) => Promise<ListSessionsResponse>
  connectSession: (id: DaemonSessionId) => Promise<DaemonSession>
  getSession: (id: DaemonSessionId) => Promise<DaemonSession>
  getHistory: (params: GetSessionHistoryRequest) => Promise<GetSessionHistoryResponse>
  getChanges: (id: DaemonSessionId) => Promise<GetSessionChangesResponse>
  getComposerSuggestions: (
    params: SessionComposerSuggestionsRequest,
  ) => Promise<SessionComposerSuggestionsResponse>
  getDraftSuggestions: (
    params: SessionDraftSuggestionsRequest,
  ) => Promise<SessionComposerSuggestionsResponse>
  getLaunchPreview: (params: SessionLaunchPreviewRequest) => Promise<SessionLaunchPreviewResponse>
  getSubpackages: (params: SessionSubpackagesRequest) => Promise<SessionSubpackagesResponse>
  getDiagnostics: (id: DaemonSessionId) => Promise<GetSessionDiagnosticsResponse>
  getWorktree: (id: SessionWorktreeIdentity["id"]) => Promise<GetSessionWorktreeResponse>
  mountReviewSession: (
    id: SessionWorktreeIdentity["id"],
  ) => Promise<MutateSessionReviewSessionResponse>
  runReviewSession: (
    id: SessionWorktreeIdentity["id"],
  ) => Promise<MutateSessionReviewSessionResponse>
  unmountReviewSession: (
    id: SessionWorktreeIdentity["id"],
  ) => Promise<MutateSessionReviewSessionResponse>
  getWorkforce: (id: DaemonSessionId) => Promise<GetSessionWorkforceResponse>
  shutdownSession: (id: DaemonSessionId) => Promise<boolean>
  cancelSessionTurn: (id: DaemonSessionId) => Promise<CancelSessionResponse>
  steerSession: (
    id: DaemonSessionId,
    prompt: SteerSessionRequest["prompt"],
  ) => Promise<SteerSessionResponse>
  sendMessage: (id: DaemonSessionId, message: SendSessionMessageRequest["message"]) => Promise<void>
  completeSession: (id: DaemonSessionId) => Promise<CompleteSessionResponse["item"]>
  declareInitiative: (id: DaemonSessionId, title: string) => Promise<DaemonSession>
  reportBlocker: (
    id: DaemonSessionId,
    reason: string,
    metadata?: SessionInboxMetadataInput,
  ) => Promise<DaemonSession>
  reportTurnEnded: (
    id: DaemonSessionId,
    metadata?: SessionInboxMetadataInput,
  ) => Promise<DaemonSession>
  recordTurnAttentionActivity: (
    id: DaemonSessionId,
    metadata?: SessionInboxMetadataInput & { fallbackHeadline?: string },
  ) => Promise<{ scope: InboxScope; headline: InboxHeadline; turnId: string | null }>
  sessionSubscriberConnected: (id: DaemonSessionId) => Promise<void>
  sessionSubscriberDisconnected: (id: DaemonSessionId) => Promise<void>
  resolveSessionIdByToken: (token: string) => Promise<DaemonSessionId>
}

/** Daemon setup context required to mount the session feature plugin. */
export type SessionSetupContext = {
  controller: SessionController
  setRequestSessionId: (id: DaemonSessionId) => void
}

/** First-class daemon extension exposed as `context.session` to consuming feature plugins. */
export function createSessionExtension(controller: SessionController) {
  return {
    create: (request: CreateSessionRequest) => controller.newSession({ request }),
    list: (params: ListSessionsRequest) => controller.listSessions(params),
    connect: (id: DaemonSessionId) => controller.connectSession(id),
    get: (id: DaemonSessionId) => controller.getSession(id),
    history: (params: GetSessionHistoryRequest) => controller.getHistory(params),
    changes: (id: DaemonSessionId) => controller.getChanges(id),
    composerSuggestions: (params: SessionComposerSuggestionsRequest) =>
      controller.getComposerSuggestions(params),
    draftSuggestions: (params: SessionDraftSuggestionsRequest) =>
      controller.getDraftSuggestions(params),
    launchPreview: (params: SessionLaunchPreviewRequest) => controller.getLaunchPreview(params),
    subpackages: (params: SessionSubpackagesRequest) => controller.getSubpackages(params),
    diagnostics: (id: DaemonSessionId) => controller.getDiagnostics(id),
    worktree: (id: SessionWorktreeIdentity["id"]) => controller.getWorktree(id),
    mountReviewSession: (id: SessionWorktreeIdentity["id"]) => controller.mountReviewSession(id),
    runReviewSession: (id: SessionWorktreeIdentity["id"]) => controller.runReviewSession(id),
    unmountReviewSession: (id: SessionWorktreeIdentity["id"]) =>
      controller.unmountReviewSession(id),
    workforce: (id: DaemonSessionId) => controller.getWorkforce(id),
    shutdown: (id: DaemonSessionId) => controller.shutdownSession(id),
    cancel: (id: DaemonSessionId) => controller.cancelSessionTurn(id),
    steer: (id: DaemonSessionId, prompt: SteerSessionRequest["prompt"]) =>
      controller.steerSession(id, prompt),
    sendMessage: (id: DaemonSessionId, message: SendSessionMessageRequest["message"]) =>
      controller.sendMessage(id, message),
    complete: (id: DaemonSessionId) => controller.completeSession(id),
    declareInitiative: (id: DaemonSessionId, title: string) =>
      controller.declareInitiative(id, title),
    reportBlocker: (id: DaemonSessionId, reason: string, metadata?: SessionInboxMetadataInput) =>
      controller.reportBlocker(id, reason, metadata),
    reportTurnEnded: (id: DaemonSessionId, metadata?: SessionInboxMetadataInput) =>
      controller.reportTurnEnded(id, metadata),
    recordTurnAttentionActivity: (
      id: DaemonSessionId,
      metadata?: SessionInboxMetadataInput & { fallbackHeadline?: string },
    ) => controller.recordTurnAttentionActivity(id, metadata),
    subscriberConnected: (id: DaemonSessionId) => controller.sessionSubscriberConnected(id),
    subscriberDisconnected: (id: DaemonSessionId) => controller.sessionSubscriberDisconnected(id),
    resolveToken: (token: string) => controller.resolveSessionIdByToken(token),
  }
}
