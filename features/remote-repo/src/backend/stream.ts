import type { BackendEventEnvelope } from "@goddard-ai/backend-plugin"

import type { RepoEvent } from "../schema.ts"

export type RemoteRepoStreamEvent = BackendEventEnvelope<"remote_repo.event.received", RepoEvent>

/** Minimal sink contract used by remote-repo server-sent-event fanout. */
export type RemoteRepoStreamSink = {
  send: (payload: string) => void
  close?: () => void
}

/** Feature-owned service boundary for ephemeral remote repository stream delivery. */
export type RemoteRepoStreamService = {
  addStreamSocket(streamKey: string, socket: unknown): void
  removeStreamSocket(streamKey: string, socket: unknown): void
  resolveEventOwner(event: RepoEvent): Promise<string | undefined> | string | undefined
}

/** Optional in-process broadcaster used by local backend servers. */
export type RemoteRepoEventBroadcaster = {
  broadcastRemoteRepoEvent(event: RemoteRepoStreamEvent): void
}

/** Returns whether a value supports the minimal send/close contract used by SSE fanout. */
export function isRemoteRepoStreamSink(value: unknown): value is RemoteRepoStreamSink {
  return (
    !!value &&
    typeof value === "object" &&
    "send" in value &&
    typeof (value as RemoteRepoStreamSink).send === "function"
  )
}

/** Returns whether a value implements remote-repo stream socket registration. */
export function isRemoteRepoStreamService(value: unknown): value is RemoteRepoStreamService {
  return (
    !!value &&
    typeof value === "object" &&
    "addStreamSocket" in value &&
    typeof (value as RemoteRepoStreamService).addStreamSocket === "function" &&
    "removeStreamSocket" in value &&
    typeof (value as RemoteRepoStreamService).removeStreamSocket === "function" &&
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
