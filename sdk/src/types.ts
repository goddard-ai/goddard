export type RepoRef = {
  owner: string;
  repo: string;
};

export type DeviceFlowStart = {
  githubUsername?: string;
};

export type DeviceFlowSession = {
  deviceCode: string;
  userCode: string;
  verificationUri: string;
  expiresIn: number;
  interval: number;
};

export type DeviceFlowComplete = {
  deviceCode: string;
  githubUsername: string;
};

export type AuthSession = {
  token: string;
  githubUsername: string;
  githubUserId: number;
};

export type CreatePrInput = RepoRef & {
  title: string;
  body?: string;
  head: string;
  base: string;
};

export type PullRequestRecord = {
  id: number;
  number: number;
  owner: string;
  repo: string;
  title: string;
  body: string;
  head: string;
  base: string;
  url: string;
  createdBy: string;
  createdAt: string;
};

export type TriggerActionInput = RepoRef & {
  workflowId: string;
  ref: string;
  inputs?: Record<string, string>;
};

export type ActionRunRecord = {
  id: number;
  owner: string;
  repo: string;
  workflowId: string;
  ref: string;
  status: "queued" | "in_progress" | "completed";
  triggeredBy: string;
  createdAt: string;
};

export type RepoEvent =
  | {
      type: "comment";
      owner: string;
      repo: string;
      prNumber: number;
      author: string;
      body: string;
      reactionAdded: "eyes";
      createdAt: string;
    }
  | {
      type: "review";
      owner: string;
      repo: string;
      prNumber: number;
      author: string;
      state: "approved" | "changes_requested" | "commented";
      body: string;
      reactionAdded: "eyes";
      createdAt: string;
    }
  | {
      type: "pr.created";
      owner: string;
      repo: string;
      prNumber: number;
      title: string;
      author: string;
      createdAt: string;
    }
  | {
      type: "action.triggered";
      owner: string;
      repo: string;
      workflowId: string;
      runId: number;
      status: "queued";
      author: string;
      createdAt: string;
    };

export type StreamMessage = {
  event: RepoEvent;
};

export type GitHubWebhookInput =
  | {
      type: "issue_comment";
      owner: string;
      repo: string;
      prNumber: number;
      author: string;
      body: string;
    }
  | {
      type: "pull_request_review";
      owner: string;
      repo: string;
      prNumber: number;
      author: string;
      state: "approved" | "changes_requested" | "commented";
      body: string;
    };
