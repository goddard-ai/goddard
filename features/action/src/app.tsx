import { defineAppPlugin } from "@goddard-ai/app-plugin"

export const actionAppPlugin = defineAppPlugin({
  name: "action",
  routes: [],
  commands: [],
  sdk: {
    namespaces: ["action"],
  },
})
