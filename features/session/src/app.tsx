import { defineAppPlugin } from "@goddard-ai/app-plugin"

import { sessionSdkPlugin } from "./sdk.ts"

/** SDK surface the app composition root must provide to session app contributions. */
export type SessionAppSdkRequirements = {
  readonly [TNamespace in typeof sessionSdkPlugin.namespace]: ReturnType<
    typeof sessionSdkPlugin.create
  >
}

export const sessionAppPlugin = defineAppPlugin({
  name: "session",
  sdk: {} as SessionAppSdkRequirements,
  navigation: {
    slot: "primaryWorkbench",
    id: "sessions",
    label: "Sessions",
    icon: "tabs/sessions",
  },
  workbenchTab: {
    kind: "sessions",
    icon: "tabs/sessions",
  },
})
