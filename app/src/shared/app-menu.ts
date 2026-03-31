/** Native menu actions that the Bun host can dispatch into the active webview. */
export type AppMenuAction = "closeTab"

/** Custom browser event name dispatched when the native menu requests one app action. */
export const APP_MENU_EVENT_NAME = "goddard:app-menu"

/** Detail payload carried by one native app-menu event. */
export type AppMenuEventDetail = {
  action: AppMenuAction
}

/** Serializes one native menu action into JavaScript that Bun can inject into the webview. */
export function createAppMenuDispatchScript(detail: AppMenuEventDetail): string {
  return `window.dispatchEvent(new CustomEvent(${JSON.stringify(APP_MENU_EVENT_NAME)}, { detail: ${JSON.stringify(detail)} }));`
}
