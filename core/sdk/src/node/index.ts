import { GoddardSdk as BaseGoddardSdk } from "../sdk.ts"
import * as actions from "../daemon/actions.ts"
import * as loops from "../daemon/loops.ts"
import * as workforce from "./workforce.ts"
export * from "./workforce.ts"

/** Goddard SDK entry point extended for local Node.js environments with daemon-oriented conveniences. */
export class GoddardSdk extends BaseGoddardSdk {
  get actions() {
    return {
      /** Runs an isolated, one-shot action agent for short-lived queries or tasks. */
      run: actions.runAgentAction,
    }
  }

  get loop() {
    return {
      /** Starts a new named background loop managed by the daemon. */
      start: loops.startNamedLoop,
      /** Retrieves the current status of a daemon-managed background loop. */
      get: loops.getLoop,
      /** Lists all daemon-managed background loops currently running. */
      list: loops.listLoops,
      /** Stops a named daemon-managed background loop. */
      stop: loops.stopLoop,
    }
  }

  get workforce() {
    return {
      /** Starts the primary daemon-managed workforce runtime for a repository. */
      start: workforce.startWorkforce,
      /** Retrieves the status of the daemon-managed workforce for a repository. */
      get: workforce.getWorkforce,
      /** Lists all active daemon-managed workforce runtimes. */
      list: workforce.listWorkforces,
      /** Stops the daemon-managed workforce runtime for a repository. */
      stop: workforce.stopWorkforce,
      /** Submits a new request for the workforce to process. */
      request: workforce.createWorkforceRequest,
      /** Updates the content or intent of an existing workforce request. */
      update: workforce.updateWorkforceRequest,
      /** Cancels a pending workforce request. */
      cancel: workforce.cancelWorkforceRequest,
      /** Truncates the history of processed workforce requests. */
      truncate: workforce.truncateWorkforce,
    }
  }
}
