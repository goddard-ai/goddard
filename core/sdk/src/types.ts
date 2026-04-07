/** SDK-facing aliases for daemon-backed request and event types. */
import type {
  CancelDaemonWorkforceRequest,
  CreateDaemonSessionRequest,
  CreateDaemonWorkforceRequestRequest,
  DaemonWorkforceEvent,
  DiscoverDaemonWorkforceCandidatesRequest,
  GetDaemonLoopRequest,
  GetDaemonWorkforceRequest,
  InitializeDaemonWorkforceRequest,
  ListDaemonSessionsRequest,
  ReplyPrDaemonRequest,
  ResolveDaemonSessionTokenRequest,
  RespondDaemonWorkforceRequest,
  RunNamedDaemonActionRequest,
  SendDaemonSessionMessageRequest,
  ShutdownDaemonLoopRequest,
  ShutdownDaemonWorkforceRequest,
  StartDaemonLoopRequest,
  StartDaemonWorkforceRequest,
  SubscribeDaemonWorkforceEventsRequest,
  SubmitPrDaemonRequest,
  SuspendDaemonWorkforceRequest,
  TruncateDaemonWorkforceRequest,
  UpdateDaemonWorkforceRequest,
} from "@goddard-ai/schema/daemon"

/** SDK input for submitting one pull request. */
export interface PrSubmitInput extends SubmitPrDaemonRequest {
  token: string
}

/** SDK input for replying to one pull request review thread. */
export interface PrReplyInput extends ReplyPrDaemonRequest {
  token: string
}

/** SDK input for creating one daemon-backed session record. */
export interface SessionCreateInput extends CreateDaemonSessionRequest {}

/** SDK input for listing daemon-backed sessions with pagination. */
export interface SessionListInput extends ListDaemonSessionsRequest {}

/** SDK input for sending one raw message to a daemon-backed session. */
export interface SessionSendInput extends SendDaemonSessionMessageRequest {}

/** SDK input for resolving one session token into a session id. */
export interface SessionResolveTokenInput extends ResolveDaemonSessionTokenRequest {}

/** SDK input for running one named action. */
export interface ActionRunInput extends RunNamedDaemonActionRequest {}

/** SDK input for starting or reusing one loop runtime. */
export interface LoopStartInput extends StartDaemonLoopRequest {}

/** SDK input for fetching one loop runtime. */
export interface LoopGetInput extends GetDaemonLoopRequest {}

/** SDK input for shutting down one loop runtime. */
export interface LoopShutdownInput extends ShutdownDaemonLoopRequest {}

/** SDK input for starting or reusing one workforce runtime. */
export interface WorkforceStartInput extends StartDaemonWorkforceRequest {}

/** SDK input for discovering workforce initialization candidates. */
export interface WorkforceDiscoverCandidatesInput extends DiscoverDaemonWorkforceCandidatesRequest {}

/** SDK input for initializing one workforce config and ledger. */
export interface WorkforceInitializeInput extends InitializeDaemonWorkforceRequest {}

/** SDK input for fetching one workforce runtime. */
export interface WorkforceGetInput extends GetDaemonWorkforceRequest {}

/** SDK callback payload for one workforce ledger event. */
export type WorkforceEvent = DaemonWorkforceEvent["event"]

/** SDK input for subscribing to one workforce event stream. */
export interface WorkforceSubscribeInput extends SubscribeDaemonWorkforceEventsRequest {}

/** SDK input for shutting down one workforce runtime. */
export interface WorkforceShutdownInput extends ShutdownDaemonWorkforceRequest {}

/** SDK input for enqueueing one workforce request. */
export interface WorkforceRequestInput extends CreateDaemonWorkforceRequestRequest {}

/** SDK input for updating one workforce request. */
export interface WorkforceUpdateInput extends UpdateDaemonWorkforceRequest {}

/** SDK input for cancelling one workforce request. */
export interface WorkforceCancelInput extends CancelDaemonWorkforceRequest {}

/** SDK input for truncating one workforce queue. */
export interface WorkforceTruncateInput extends TruncateDaemonWorkforceRequest {}

/** SDK input for responding to one active workforce request. */
export interface WorkforceRespondInput extends RespondDaemonWorkforceRequest {}

/** SDK input for suspending one active workforce request. */
export interface WorkforceSuspendInput extends SuspendDaemonWorkforceRequest {}
