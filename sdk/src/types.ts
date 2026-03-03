export type ThinkingLevel = "off" | "minimal" | "low" | "medium" | "high" | "xhigh";

/**
 * Configuration passed through to pi-coding-agent when creating a session.
 * Supports extra provider-specific fields via the index signature.
 */
export type PiAgentConfig = {
  /** pi-coding-agent model string, e.g. "anthropic/claude-opus-4-5" */
  model: string;
  /** Absolute path to the project the agent should operate in */
  projectDir: string;
  /** Thinking / reasoning depth. Defaults to "medium". */
  thinkingLevel?: ThinkingLevel;
  /** Override path to the pi agent directory (~/.pi/agent by default) */
  agentDir?: string;
  /**
   * System prompt to inject into the agent session.
   * Defaults to LOOP_SYSTEM_PROMPT when running via `goddard loop`.
   * Pass SPEC_SYSTEM_PROMPT when running via `goddard spec`.
   */
  systemPrompt?: string;
  /** Allow provider-specific pass-through fields */
  [key: string]: unknown;
};

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
