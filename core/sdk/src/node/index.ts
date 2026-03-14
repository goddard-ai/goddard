import { GoddardSdk, type GoddardSdkOptions } from "../index.ts"
import * as agents from "./agents.ts"
import * as loop from "./loop.ts"
export { FileTokenStorage } from "@goddard-ai/storage"

export class NodeGoddardSdk extends GoddardSdk {
  constructor(options: GoddardSdkOptions) {
    super(options)

    ;(this as any).agents = {
      init: agents.init,
    }

    ;(this as any).loop = {

      run: loop.runLoop,

    }
  }
}

export function createSdk(options: GoddardSdkOptions): NodeGoddardSdk {
  return new NodeGoddardSdk(options)
}

export * from "./agents.ts"
export * from "./loop.ts"
