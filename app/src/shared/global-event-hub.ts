import type { InferStreamPayload, ValidStreamName } from "@goddard-ai/ipc";
import type { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc";
import { SigmaTarget } from "preact-sigma";

import type { AppCommandId } from "./app-commands.ts";
import type { DebugMenuSurface } from "./debug-menu.ts";
type DaemonSchema = typeof daemonIpcSchema;

/** One daemon IPC stream name that the app bridge can forward from the Bun host. */
export type DaemonStreamName = ValidStreamName<DaemonSchema>;

/** One daemon stream payload envelope dispatched from the Bun host into the active webview. */
export type DaemonStreamEventDetail<Name extends DaemonStreamName = DaemonStreamName> = {
  subscriptionId: string;
  name: Name;
  payload: InferStreamPayload<DaemonSchema, Name>;
};

/** Shared typed global events dispatched across the active webview. */
export type GlobalEvents = {
  appMenu: { command: AppCommandId };
  commandDialogActivated: { dialogId: string };
  daemonStream: DaemonStreamEventDetail;
  debugMenu: { surface: DebugMenuSurface };
};

/** One supported global event name. */
export type GlobalEventName = keyof GlobalEvents;

/** Payload for one supported global event name. */
export type GlobalEventDetail<Name extends GlobalEventName> = GlobalEvents[Name];

/** One app-wide global event dispatched through the shared singleton hub. */
export type GlobalEventEnvelope<Name extends GlobalEventName = GlobalEventName> = {
  [EventName in Name]: {
    name: EventName;
    detail: GlobalEventDetail<EventName>;
  };
}[Name];

class GlobalEventHub extends SigmaTarget<GlobalEvents> {
  dispatch(event: GlobalEventEnvelope) {
    switch (event.name) {
      case "appMenu":
        this.emit("appMenu", event.detail);
        return;
      case "commandDialogActivated":
        this.emit("commandDialogActivated", event.detail);
        return;
      case "daemonStream":
        this.emit("daemonStream", event.detail);
        return;
      case "debugMenu":
        this.emit("debugMenu", event.detail);
        return;
    }
  }
}

export const globalEventHub = new GlobalEventHub();

/** Dispatches one typed global event on the singleton hub. */
export function dispatchGlobalEvent(event: GlobalEventEnvelope) {
  globalEventHub.dispatch(event);
}
