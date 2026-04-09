import { SigmaType } from "preact-sigma"
import { DebugMenuSurface } from "./debug-menu.ts"

type GlobalEvents = {
  "open-debug-surface": { surface: DebugMenuSurface }
}

// A hacky but acceptable typed event target for global app events.
export const globalEventHub = new new SigmaType<never, GlobalEvents>()()

/** Serializes one event-hub dispatch into JavaScript that Bun can inject into the webview. */
export function createGlobalEventDispatchScript<TDetail>(
  eventName: string,
  detail: TDetail,
): string {
  return `(() => {
    const key = ${JSON.stringify(GLOBAL_EVENT_HUB_KEY)};
    const globalEventHubOwner = globalThis;
    const globalEventHub = globalEventHubOwner[key] ?? (globalEventHubOwner[key] = new EventTarget());
    globalEventHub.dispatchEvent(new CustomEvent(${JSON.stringify(eventName)}, { detail: ${JSON.stringify(detail)} }));
  })();`
}

/** Adapts one typed detail listener to the generic event callback shape used by EventTarget listeners. */
export function createGlobalEventDetailListener<TDetail>(listener: (detail: TDetail) => void) {
  return (event: Event) => {
    listener((event as CustomEvent<TDetail>).detail)
  }
}
