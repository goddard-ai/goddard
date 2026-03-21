export type ProviderId =
  | "google-jules"
  | "openai-codex-cloud"
  | "cursor-cloud";

export type Repo =
  | { type: "github"; owner: string; repo: string; branch?: string }
  | { type: "local"; path: string };

export type AgentJobStatus =
  | "running"
  | "completed"
  | "failed";

export interface AgentJobRequest {
  prompt: string;
  repo: Repo;
}

export interface AgentJob {
  id: string;
  provider: ProviderId;
  status: AgentJobStatus;
}

export interface AgentJobResult {
  success: boolean;
  summary?: string;
  url?: string;
  patch?: string;
  error?: string;
}

export interface AgentProvider {
  readonly id: ProviderId;
  startJob(request: AgentJobRequest): Promise<AgentJob>;
  getJob(jobId: string): Promise<AgentJob>;
  getResult(jobId: string): Promise<AgentJobResult>;
}
