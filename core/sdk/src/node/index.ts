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
      discoverPackages: workforce.discoverWorkforcePackages,
      discoverInitCandidates: workforce.discoverWorkforceInitCandidates,
      initializePackages: workforce.initializeWorkforcePackages,
      watch: workforce.watchWorkforce,
    }
  }
}
