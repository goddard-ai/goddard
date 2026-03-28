import { AgentJob, AgentJobRequest, AgentJobResult, AgentProvider } from "../types";

export class JulesProvider implements AgentProvider {
  readonly id = "google-jules";

  async startJob(request: AgentJobRequest): Promise<AgentJob> {
    const res = await fetch("https://api.jules.google.com/agents", {
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
    const res = await fetch(`https://api.jules.google.com/agents/${jobId}`);
    const data = await res.json();

    return {
      id: jobId,
      provider: this.id,
      status: data.status
    };
  }

  async getResult(jobId: string): Promise<AgentJobResult> {
    const res = await fetch(`https://api.jules.google.com/agents/${jobId}/result`);
    return res.json();
  }
}
