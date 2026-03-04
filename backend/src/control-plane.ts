import type {
  AuthSession,
  CreatePrInput,
  DeviceFlowComplete,
  DeviceFlowSession,
  DeviceFlowStart,
  GitHubWebhookInput,
  PullRequestRecord,
  RepoEvent
} from "@goddard-ai/schema";

export interface BackendControlPlane {
  startDeviceFlow(input?: DeviceFlowStart): Promise<DeviceFlowSession> | DeviceFlowSession;
  completeDeviceFlow(input: DeviceFlowComplete): Promise<AuthSession> | AuthSession;
  getSession(token: string): Promise<AuthSession> | AuthSession;
  createPr(token: string, input: CreatePrInput): Promise<PullRequestRecord> | PullRequestRecord;
  isManagedPr(owner: string, repo: string, prNumber: number): Promise<boolean> | boolean;
  handleGitHubWebhook(event: GitHubWebhookInput): Promise<RepoEvent> | RepoEvent;
  addStreamSocket?(repoKey: string, socket: unknown): void;
  removeStreamSocket?(repoKey: string, socket: unknown): void;
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
