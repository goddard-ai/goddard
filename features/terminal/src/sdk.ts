import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

export const terminalSdkPlugin = defineSdkPlugin({
  name: "terminal",
  namespace: "terminal",
  create() {
    return {}
  },
})
