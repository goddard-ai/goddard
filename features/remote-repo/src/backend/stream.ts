import type { BackendEventStreamRequest, RepoEvent } from "../schema.ts"

/** Feature-owned service boundary for ephemeral remote repository stream delivery. */
export type RemoteRepoStreamService = {
  subscribeRemoteRepoEvents(
    streamKey: string,
    filter?: BackendEventStreamRequest,
  ): AsyncIterable<RepoEvent>
  resolveEventOwner(event: RepoEvent): Promise<string | undefined> | string | undefined
}

/** Optional in-process broadcaster used by local backend servers. */
export type RemoteRepoEventBroadcaster = {
  broadcastRemoteRepoEvent(event: RemoteRepoStreamEvent): void
}

/** Returns whether a value implements remote-repo stream socket registration. */
export function isRemoteRepoStreamService(value: unknown): value is RemoteRepoStreamService {
  return (
    !!value &&
    typeof value === "object" &&
    "subscribeRemoteRepoEvents" in value &&
    typeof (value as RemoteRepoStreamService).subscribeRemoteRepoEvents === "function" &&
    "resolveEventOwner" in value &&
    typeof (value as RemoteRepoStreamService).resolveEventOwner === "function"
  )
}

/** Returns whether a value can publish remote-repo events to local stream sinks. */
export function isRemoteRepoEventBroadcaster(value: unknown): value is RemoteRepoEventBroadcaster {
  return (
    !!value &&
    typeof value === "object" &&
    "broadcastRemoteRepoEvent" in value &&
    typeof (value as RemoteRepoEventBroadcaster).broadcastRemoteRepoEvent === "function"
  )
}
