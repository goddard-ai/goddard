import { terminalRootClass } from "./app.style.ts"

export const terminalAppPlugin = {
  name: "terminal",
  routes: [],
  commands: [],
  sdk: {
    namespaces: ["terminal"],
  },
  styles: {
    rootClass: terminalRootClass,
  },
} as const
