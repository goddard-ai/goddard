import { GoddardSdk as BaseGoddardSdk } from "../sdk.js"
import * as actions from "./actions.js"
import * as agents from "./agents.js"
import * as loops from "./loops.js"
import * as workforce from "./workforce.js"
export { FileTokenStorage } from "@goddard-ai/storage"
export * from "./workforce.js"

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
      resolve: loops.resolveLoop,
      run: loops.runAdHocLoop,
      runNamed: loops.runNamedLoop,
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
