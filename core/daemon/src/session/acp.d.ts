import * as acp from "@agentclientprotocol/sdk";
import { Readable, Writable } from "node:stream";
export type AnyRequest = acp.AnyMessage & {
    params: unknown;
};
/** Optional callbacks used to observe raw agent stream traffic. */
export type AgentStreamHooks = {
    onChunk?: (chunk: Uint8Array) => void;
    onMessageError?: (error: unknown) => void;
};
export declare function isAcpRequest<T extends AnyRequest>(message: {
    jsonrpc?: string;
}, method: string): message is T;
export declare function matchAcpRequest<T>(message: acp.AnyMessage, method: string): T | null;
export declare function getAcpMessageResult<T>(message: acp.AnyMessage): T | null;
export declare function createAgentConnection(stdin: Writable, stdout: Readable, hooks?: AgentStreamHooks): {
    getWriter(): WritableStreamDefaultWriter<acp.AnyMessage>;
    subscribe(onMessage: (message: acp.AnyMessage) => Promise<void>): {
        closed: Promise<void>;
        close(): Promise<void>;
    };
};
export declare function createAgentMessageStream(stdin: Writable, stdout: Readable, hooks?: AgentStreamHooks): acp.Stream;
//# sourceMappingURL=acp.d.ts.map