import type { WorkforceLedgerEvent, WorkforceProjection, WorkforceRequestRecord } from "@goddard-ai/schema/workforce";
export declare function summarizeWorkforceProjection(requests: Record<string, WorkforceRequestRecord>): {
    activeRequestCount: number;
    queuedRequestCount: number;
    suspendedRequestCount: number;
    failedRequestCount: number;
};
export declare function buildWorkforceQueues(requests: Record<string, WorkforceRequestRecord>): Record<string, string[]>;
export declare function applyWorkforceEvent(requests: Record<string, WorkforceRequestRecord>, event: WorkforceLedgerEvent): Record<string, WorkforceRequestRecord>;
export declare function readWorkforceLedger(rootDir: string): Promise<WorkforceLedgerEvent[]>;
export declare function appendWorkforceLedgerEvent(rootDir: string, event: WorkforceLedgerEvent): Promise<void>;
export declare function replayWorkforceProjection(rootDir: string): Promise<WorkforceProjection>;
//# sourceMappingURL=ledger.d.ts.map