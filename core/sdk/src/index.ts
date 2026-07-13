export * from "./ipc-client.ts"
export * from "./agent.ts"
export * from "./session.ts"
export * from "./sdk.ts"
export * from "@goddard-ai/terminal/sdk"
export * from "@goddard-ai/vscode-task/schema"
export * from "@goddard-ai/vscode-task/sdk"
export { AgentSession } from "./daemon/session/client-session.ts"
export type {
  DaemonEventEnvelope,
  DaemonEventOptions,
  DaemonEventPropertyFilter,
  DaemonEventsStreamRequest,
  GetUserConfigResponse,
  UpdateUserConfigRequest,
  UpdateUserConfigResponse,
  UserConfigDocument,
  UserConfigIpcError,
  UserConfigJsonSchema,
} from "@goddard-ai/schema/daemon-ipc"
export { UserConfigErrorCodes, UserConfigIpcErrors } from "@goddard-ai/schema/daemon-ipc"
export type {
  FileSearchComposerEntriesRequest,
  FileSearchComposerEntriesResponse,
  FileSearchComposerEntry,
} from "@goddard-ai/file-search/schema"
