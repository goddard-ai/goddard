import { GoddardSdk as BaseGoddardSdk } from "../sdk.ts"
import { runAgentLoop } from "../loop/run-agent-loop.ts"
import * as actions from "./actions.ts"
import * as agents from "./agents.ts"
import * as workforce from "./workforce.ts"
export { FileTokenStorage } from "@goddard-ai/storage"
export * from "./workforce.ts"

export class GoddardSdk extends BaseGoddardSdk {
  get agents() {
    return {
      init: agents.init,
    }
  }

  get actions() {
    return {
      run: actions.runAgentAction,
    }
  }

  get loop() {
    return {
      run: runAgentLoop,
    }
  }

  get workforce() {
    return {
      resolveRepositoryRoot: workforce.resolveRepositoryRoot,
      discoverInitCandidates: workforce.discoverWorkforceInitCandidates,
      initialize: workforce.initializeWorkforce,
      start: workforce.startWorkforce,
      get: workforce.getWorkforce,
      list: workforce.listWorkforces,
      stop: workforce.stopWorkforce,
      request: workforce.createWorkforceRequest,
      update: workforce.updateWorkforceRequest,
      cancel: workforce.cancelWorkforceRequest,
      truncate: workforce.truncateWorkforce,
    }
  }
}
