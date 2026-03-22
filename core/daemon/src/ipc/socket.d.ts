import { createDaemonUrl, readSocketPathFromDaemonUrl } from "@goddard-ai/schema/daemon-url";
export { createDaemonUrl, readSocketPathFromDaemonUrl };
export declare function getDefaultDaemonSocketPath(): string;
export declare function prepareSocketPath(socketPath: string): Promise<void>;
export declare function cleanupSocketPath(socketPath: string): Promise<void>;
//# sourceMappingURL=socket.d.ts.map