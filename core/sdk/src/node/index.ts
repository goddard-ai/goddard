import * as loop from "@goddard-ai/loop"
import * as sdk from "../index.ts"
import * as actions from "./actions.ts"
import * as agents from "./agents.ts"
export { FileTokenStorage } from "@goddard-ai/storage"

export class GoddardSdk extends sdk.GoddardSdk {
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
      run: loop.runAgentLoop,
    }
  }
}
