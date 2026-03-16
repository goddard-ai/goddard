import * as loop from "@goddard-ai/loop"
import { GoddardSdk, type GoddardSdkOptions } from "../index.ts"
import type { RunAgentActionParams } from "./actions.ts"
import * as actions from "./actions.ts"
import * as agents from "./agents.ts"
export { FileTokenStorage } from "@goddard-ai/storage"

export interface NodeGoddardAgentsApi {
  init: typeof agents.init
  runAgentAction: (action: string, options: RunAgentActionParams) => Promise<any>
}

export class NodeGoddardSdk extends GoddardSdk {
  declare readonly agents: NodeGoddardAgentsApi
  declare readonly loop: {
    run: typeof loop.runAgentLoop
  }

  constructor(options: GoddardSdkOptions) {
    super(options)

    this.agents = {
      init: agents.init,
      runAgentAction: actions.runAgentAction,
    }

    this.loop = {
      run: loop.runAgentLoop,
    }
  }
}

export function createSdk(options: GoddardSdkOptions): NodeGoddardSdk {
  return new NodeGoddardSdk(options)
}

export * from "./actions.ts"
export * from "./agents.ts"
