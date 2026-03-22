import * as acp from "@agentclientprotocol/sdk";
import type { CreateDaemonSessionRequest, DaemonSession, GetDaemonSessionDiagnosticsResponse, GetDaemonSessionHistoryResponse } from "@goddard-ai/schema/daemon";
/** Exposes the daemon operations for creating, connecting to, and controlling sessions. */
export type SessionManager = {
    createSession: (params: CreateDaemonSessionRequest) => Promise<DaemonSession>;
    connectSession: (id: string) => Promise<DaemonSession>;
    getSession: (id: string) => Promise<DaemonSession>;
    getHistory: (id: string) => Promise<GetDaemonSessionHistoryResponse>;
    getDiagnostics: (id: string) => Promise<GetDaemonSessionDiagnosticsResponse>;
    sendMessage: (id: string, message: acp.AnyMessage) => Promise<void>;
    promptSession: (id: string, prompt: string | acp.ContentBlock[]) => Promise<acp.PromptResponse>;
    shutdownSession: (id: string) => Promise<boolean>;
    resolveSessionIdByToken: (token: string) => Promise<string>;
    close: () => Promise<void>;
};
/** Ensures the daemon's system prompt is prepended to the first user prompt sent to an agent. */
export declare function injectSystemPrompt(request: acp.PromptRequest, systemPrompt: string): acp.PromptRequest;
export declare function createSessionManager(input: {
    daemonUrl: string;
    agentBinDir: string;
    publish: (id: string, message: acp.AnyMessage) => void;
}): SessionManager;
//# sourceMappingURL=manager.d.ts.map