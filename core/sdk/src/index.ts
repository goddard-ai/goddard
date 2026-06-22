export * from "./ipc-client.ts"
export * from "./managed-agent.ts"
export * from "./session.ts"
export * from "./sdk.ts"
export { AgentSession } from "./daemon/session/client-session.ts"
export type {
  DaemonEventEnvelope,
  DaemonEventLogMetadata,
  DaemonEventPropertyFilter,
  DaemonEventsStreamRequest,
} from "@goddard-ai/schema/daemon-ipc"
export type {
  FileSearchComposerEntriesRequest,
  FileSearchComposerEntriesResponse,
  FileSearchComposerEntry,
} from "@goddard-ai/file-search/schema"
