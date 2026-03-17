import { GoddardSdk as BaseGoddardSdk } from "../sdk.ts"
import { runAgentLoop } from "../loop/run-agent-loop.ts"
import * as actions from "./actions.ts"
import * as agents from "./agents.ts"
export { FileTokenStorage } from "@goddard-ai/storage"

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
}
