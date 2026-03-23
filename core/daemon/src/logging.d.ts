/** Function that writes a single serialized daemon log line. */
export type DaemonLogWriter = (line: string) => void
/** Supported terminal output modes for daemon logs. */
export type DaemonLogMode = "json" | "pretty" | "verbose"
/** Structured preview emitted when long text must be truncated for logs. */
export type DaemonTextPreview = {
  text: string
  byteLength: number
  truncated: boolean
}
/** Sanitization settings used for payload and message previews. */
export type DaemonSanitizeOptions = {
  maxStringLength?: number
  parentKey?: string
}
/** Configures the shared daemon log writer and output mode for the current process. */
export declare function configureDaemonLogging(options: {
  writeLine?: DaemonLogWriter
  mode?: DaemonLogMode
}): () => void
/** Creates a daemon logger that follows the current global output mode. */
export declare function createDaemonLogger(writeLine?: DaemonLogWriter): {
  log(event: string, fields?: Record<string, unknown>): void
  createOpId(): `${string}-${string}-${string}-${string}-${string}`
}
/** Returns true when daemon logs are rendered in expanded verbose mode. */
export declare function isVerboseDaemonLogging(): boolean
export declare function createPayloadPreview(
  value: unknown,
  options?: DaemonSanitizeOptions,
): unknown
export declare function createChunkPreview(value: Uint8Array): DaemonTextPreview
export declare function readSessionIdForLog(value: unknown): string | undefined
//# sourceMappingURL=logging.d.ts.map
