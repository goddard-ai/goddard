import { AgentJobRequest, AgentProvider } from "./types";
import { CursorProvider } from "./providers/cursor";
import { JulesProvider } from "./providers/jules";
import { CodexProvider } from "./providers/codex";

export async function subscribeToAgent(provider: AgentProvider, jobId: string) {
  while (true) {
    const state = await provider.getJob(jobId);

    if (state.status === "completed" || state.status === "failed") {
      return provider.getResult(jobId);
    }

    await new Promise(r => setTimeout(r, 5000));
  }
}

export async function runAgent(provider: AgentProvider, request: AgentJobRequest) {
  const job = await provider.startJob(request);
  return subscribeToAgent(provider, job.id);
}

export const providers = {
  "cursor-cloud": new CursorProvider(),
  "google-jules": new JulesProvider(),
  "openai-codex-cloud": new CodexProvider()
};
