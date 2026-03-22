import type { ReplyPrDaemonRequest, SubmitPrDaemonRequest } from "@goddard-ai/schema/daemon";
import type { ManagedPrLocationRecord } from "@goddard-ai/storage/managed-pr-locations";
import type { SessionPermissionsRecord } from "@goddard-ai/storage/session-permissions";
import type { SessionManager } from "../session/index.js";
import type { WorkforceManager } from "../workforce/index.js";
import type { LoopManager } from "../loop/index.js";
export type { ReplyPrDaemonRequest, SubmitPrDaemonRequest };
export type PrCreateInput = {
    owner: string;
    repo: string;
    title: string;
    body?: string;
    head: string;
    base: string;
};
export type PrReplyInput = {
    owner: string;
    repo: string;
    prNumber: number;
    body: string;
};
export type BackendPrClient = {
    pr: {
        create: (input: PrCreateInput) => Promise<{
            number: number;
            url: string;
        }>;
        reply: (input: PrReplyInput) => Promise<{
            success: boolean;
        }>;
    };
};
export type DaemonServer = {
    daemonUrl: string;
    socketPath: string;
    close: () => Promise<void>;
};
export type AuthorizedSession = Pick<SessionPermissionsRecord, "sessionId" | "owner" | "repo" | "allowedPrNumbers">;
export type DaemonServerDeps = {
    resolveSubmitRequest?: (input: SubmitPrDaemonRequest) => Promise<PrCreateInput>;
    resolveReplyRequest?: (input: ReplyPrDaemonRequest) => Promise<PrReplyInput>;
    getSessionByToken?: (token: string) => Promise<AuthorizedSession | null>;
    addAllowedPrToSession?: (sessionId: string, prNumber: number) => Promise<void>;
    recordManagedPrLocation?: (record: Omit<ManagedPrLocationRecord, "updatedAt">) => Promise<ManagedPrLocationRecord>;
    createLoopManager?: (input: {
        sessionManager: SessionManager;
    }) => LoopManager;
    createWorkforceManager?: (input: {
        sessionManager: SessionManager;
    }) => WorkforceManager;
};
//# sourceMappingURL=types.d.ts.map