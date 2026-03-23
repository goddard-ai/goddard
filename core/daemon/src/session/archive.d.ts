/** Marker file written after a binary payload has been fully installed into its cache directory. */
export declare const binaryInstallMarkerFileName = ".goddard-installed"
/** Supported remote payload formats for archive-backed ACP agent binaries. */
export type BinaryTargetPayloadFormat = "zip" | "tar.gz" | "tgz" | "tar.bz2" | "tbz2" | "raw"
/** Input contract for streaming one binary payload into a cache directory. */
export type InstallBinaryTargetPayloadInput = {
  archiveUrl: string
  cmd: string
  installDir: string
}
/** Detects how a binary payload should be unpacked based on its URL pathname. */
export declare function detectBinaryTargetPayloadFormat(
  archiveUrl: string,
): BinaryTargetPayloadFormat
/** Downloads one binary payload and unpacks it into the provided install directory. */
export declare function installBinaryTargetPayload(
  input: InstallBinaryTargetPayloadInput,
): Promise<void>
/** Picks the extracted archive root that relative `cmd` paths should resolve against. */
export declare function resolveInstalledBinaryRoot(installDir: string): Promise<string>
/** Resolves a binary command path from the install root, falling back to a single extracted top-level directory when needed. */
export declare function resolveInstalledBinaryCommand(
  installDir: string,
  cmd: string,
): Promise<string>
//# sourceMappingURL=archive.d.ts.map
