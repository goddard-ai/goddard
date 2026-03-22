/** Canonical filesystem paths used by one daemon-managed workforce runtime. */
export interface WorkforcePaths {
    rootDir: string;
    goddardDir: string;
    configPath: string;
    ledgerPath: string;
}
export declare function normalizeWorkforceRootDir(rootDir: string): Promise<string>;
export declare function buildWorkforcePaths(rootDir: string): WorkforcePaths;
//# sourceMappingURL=paths.d.ts.map