import { defineAppPlugin } from "@goddard-ai/app-plugin"

import { inboxSdkPlugin } from "./sdk.ts"

export type InboxAppSdkRequirements = {
  readonly [TNamespace in typeof inboxSdkPlugin.namespace]: ReturnType<typeof inboxSdkPlugin.create>
}

export const inboxAppPlugin = defineAppPlugin({
  name: "inbox",
  sdk: {} as InboxAppSdkRequirements,
  navigation: {
    slot: "primaryWorkbench",
    id: "inbox",
    label: "Inbox",
    icon: "tabs/inbox",
  },
  workbenchTab: {
    kind: "inbox",
    icon: "tabs/inbox",
  },
  commands: {
    openNavigation: {
      id: "navigation.openInbox",
      label: "Open Inbox",
      targetNavId: "inbox",
    },
  },
})
