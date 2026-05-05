import type { InferStreamPayload, ValidStreamName } from "@goddard-ai/ipc"
import type { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc"
import { SigmaTarget } from "preact-sigma"

import type { AppCommandId } from "./app-commands.ts"
import type { DebugMenuSurface } from "./debug-menu.ts"

type DaemonSchema = typeof daemonIpcSchema

/** One daemon IPC stream name that the app bridge can forward from the Bun host. */
export type DaemonStreamName = ValidStreamName<DaemonSchema>

/** One daemon stream payload envelope dispatched from the Bun host into the active webview. */
export type DaemonStreamEventDetail<Name extends DaemonStreamName = DaemonStreamName> = {
  subscriptionId: string
  name: Name
  payload: InferStreamPayload<DaemonSchema, Name>
}

/** Shared typed global events dispatched across the active webview. */
export type GlobalEvents = {
  appMenu: { command: AppCommandId }
  commandDialogActivated: { dialogId: string }
  daemonStream: DaemonStreamEventDetail
  debugMenu: { surface: DebugMenuSurface }
}

/** One supported global event name. */
export type GlobalEventName = keyof GlobalEvents

/** Payload for one supported global event name. */
export type GlobalEventDetail<Name extends GlobalEventName> = GlobalEvents[Name]

/** One app-wide global event dispatched through the shared singleton hub. */
export type GlobalEventEnvelope<Name extends GlobalEventName = GlobalEventName> = {
  [EventName in Name]: {
    name: EventName
    detail: GlobalEventDetail<EventName>
  }
}[Name]

export const globalEventHub = new SigmaTarget<GlobalEvents>()
