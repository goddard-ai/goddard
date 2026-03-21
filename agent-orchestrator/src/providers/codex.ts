import { AgentJob, AgentJobRequest, AgentJobResult, AgentProvider } from "../types";

export class CodexProvider implements AgentProvider {
  readonly id = "openai-codex-cloud";

  async startJob(request: AgentJobRequest): Promise<AgentJob> {
    const res = await fetch("https://api.openai.com/v1/codex/agents", {
      method: "POST",
      body: JSON.stringify(request)
    });

    const data = await res.json();

    return {
      id: data.id,
      provider: this.id,
      status: "running"
    };
  }

  async getJob(jobId: string): Promise<AgentJob> {
    const res = await fetch(`https://api.openai.com/v1/codex/agents/${jobId}`);
    const data = await res.json();

    return {
      id: jobId,
      provider: this.id,
      status: data.status
    };
  }

  async getResult(jobId: string): Promise<AgentJobResult> {
    const res = await fetch(`https://api.openai.com/v1/codex/agents/${jobId}/result`);
    return res.json();
  }
}
