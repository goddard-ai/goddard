import { type AgentLoopHandler, type AgentLoopParams } from "@goddard-ai/loop"
import { GoddardSdk, type GoddardSdkOptions } from "../index.ts"
import * as actions from "./actions.ts"
import type { RunAgentActionParams } from "./actions.ts"
import * as agents from "./agents.ts"
import * as loop from "./loop.ts"
export { FileTokenStorage } from "@goddard-ai/storage"

export interface NodeGoddardAgentsApi {
  init: typeof agents.init
  runAgentAction: (action: string, options: RunAgentActionParams) => Promise<any>
}

export interface NodeGoddardLoopRunOverrides {
  session?: Partial<AgentLoopParams["session"]>
  strategy?: AgentLoopParams["strategy"]
  rateLimits?: AgentLoopParams["rateLimits"]
  retries?: AgentLoopParams["retries"]
}

export interface NodeGoddardLoopApi {
  init: typeof loop.initLoopConfig
  run: (
    cwd?: string,
    overrides?: NodeGoddardLoopRunOverrides,
    handler?: AgentLoopHandler,
  ) => Promise<void>
  generateSystemdService: typeof loop.generateLoopSystemdService
}

export class NodeGoddardSdk extends GoddardSdk {
  declare readonly agents: NodeGoddardAgentsApi
  declare readonly loop: NodeGoddardLoopApi

  constructor(options: GoddardSdkOptions) {
    super(options)

    this.agents = {
      init: agents.init,
      runAgentAction: actions.runAgentAction,
    }

    this.loop = {
      init: loop.initLoopConfig,
      run: loop.runAgentLoop,
      generateSystemdService: loop.generateLoopSystemdService,
    }
  }
}

export * from "./agents.ts"
export * from "./actions.ts"
export * from "./loop.ts"
