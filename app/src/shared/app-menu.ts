import type { ShortcutCommandId } from "./shortcut-keymap.ts"

/** Native menu actions that the Bun host can dispatch into the active webview. */
export type AppMenuAction = ShortcutCommandId

/** Payload carried by one native app-menu event. */
export type AppMenuEventDetail = {
  action: AppMenuAction
}
