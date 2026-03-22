/** Canonical daemon loop identity fields derived from one repository root and loop name. */
export interface DaemonLoopIdentity {
    rootDir: string;
    loopName: string;
}
/** Normalizes the repository root used to key daemon-owned loop runtimes. */
export declare function normalizeLoopRootDir(rootDir: string): Promise<string>;
/** Builds the canonical daemon loop identity for one runtime key. */
export declare function normalizeLoopIdentity(rootDir: string, loopName: string): Promise<DaemonLoopIdentity>;
//# sourceMappingURL=paths.d.ts.map