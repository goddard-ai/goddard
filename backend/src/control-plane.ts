import type {
  ActionRunRecord,
  AuthSession,
  CreatePrInput,
  DeviceFlowComplete,
  DeviceFlowSession,
  DeviceFlowStart,
  GitHubWebhookInput,
  PullRequestRecord,
  RepoEvent,
  TriggerActionInput
} from "@goddard-ai/sdk";

export interface BackendControlPlane {
  startDeviceFlow(input?: DeviceFlowStart): Promise<DeviceFlowSession> | DeviceFlowSession;
  completeDeviceFlow(input: DeviceFlowComplete): Promise<AuthSession> | AuthSession;
  getSession(token: string): Promise<AuthSession> | AuthSession;
  createPr(token: string, input: CreatePrInput): Promise<PullRequestRecord> | PullRequestRecord;
  triggerAction(token: string, input: TriggerActionInput): Promise<ActionRunRecord> | ActionRunRecord;
  handleGitHubWebhook(event: GitHubWebhookInput): Promise<RepoEvent> | RepoEvent;
}

export class HttpError extends Error {
  constructor(
    readonly statusCode: number,
    message: string
  ) {
    super(message);
  }
}

export function assertRepo(owner: string, repo: string): void {
  if (!owner?.trim() || !repo?.trim()) {
    throw new HttpError(400, "owner and repo are required");
  }
}
