import type { DaemonWorkforce, DaemonWorkforceStatus } from "@goddard-ai/schema/daemon";
import { WorkforceRuntime, type WorkforceActorContext, type WorkforceRuntimeDeps } from "./runtime.js";
/** Supported daemon-side workforce mutations routed over IPC or agent tools. */
export type WorkforceManagerMutation = {
    type: "request";
    targetAgentId: string;
    input: string;
    intent?: "default" | "create";
} | {
    type: "update";
    requestId: string;
    input: string;
} | {
    type: "cancel";
    requestId: string;
    reason: string | null;
} | {
    type: "truncate";
    agentId: string | null;
    reason: string | null;
} | {
    type: "respond";
    requestId: string;
    output: string;
} | {
    type: "suspend";
    requestId: string;
    reason: string;
};
/** Optional lifecycle dependencies used to build new runtime instances. */
export interface WorkforceManagerDeps extends WorkforceRuntimeDeps {
    createRuntime?: (rootDir: string, deps: WorkforceRuntimeDeps) => Promise<WorkforceRuntime>;
}
/** Daemon-owned runtime registry keyed by normalized repository root. */
export interface WorkforceManager {
    startWorkforce: (rootDir: string) => Promise<DaemonWorkforce>;
    getWorkforce: (rootDir: string) => Promise<DaemonWorkforce>;
    listWorkforces: () => Promise<DaemonWorkforceStatus[]>;
    shutdownWorkforce: (rootDir: string) => Promise<boolean>;
    appendWorkforceEvent: (rootDir: string, mutation: WorkforceManagerMutation, actor?: WorkforceActorContext) => Promise<{
        workforce: DaemonWorkforceStatus;
        requestId: string | null;
    }>;
    close: () => Promise<void>;
}
export declare function createWorkforceManager(deps: WorkforceManagerDeps): WorkforceManager;
//# sourceMappingURL=manager.d.ts.map