import { defineBackendEvents, type BackendEventEnvelope } from "@goddard-ai/backend-plugin"

import { RepoEvent } from "../schema.ts"

export type RemoteRepoBackendEvent = BackendEventEnvelope<"remote_repo.event.received", RepoEvent>

export const remoteRepoBackendEvents = defineBackendEvents({
  "remote_repo.event.received": {
    payload: RepoEvent,
  },
})

/** Feature-owned backend handler for normalized remote repository events. */
export type RemoteRepoEventHandler = {
  name: string
  canHandle?: (event: RepoEvent) => boolean
  handle: (event: RepoEvent) => Promise<void> | void
}

/** Preserves the handler object while constraining it to the remote-repo event contract. */
export function defineRemoteRepoEventHandler<const THandler extends RemoteRepoEventHandler>(
  handler: THandler,
) {
  return handler
}

/** Dispatches one normalized remote repository event to interested feature handlers. */
export async function dispatchRemoteRepoEvent(
  event: RepoEvent,
  handlers: readonly RemoteRepoEventHandler[],
) {
  for (const handler of handlers) {
    if (handler.canHandle && !handler.canHandle(event)) {
      continue
    }

    await handler.handle(event)
  }
}
