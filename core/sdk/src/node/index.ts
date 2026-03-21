import { GoddardSdk as BaseGoddardSdk } from "../sdk.js"
import * as actions from "./actions.js"
import * as agents from "./agents.js"
import * as loops from "./loops.js"
import * as workforce from "./workforce.js"
export { FileTokenStorage } from "@goddard-ai/storage"
export * from "./workforce.js"

/** Goddard SDK entry point extended for local Node.js environments with file access and daemon management. */
export class GoddardSdk extends BaseGoddardSdk {
  get agents() {
    return {
      /** Initializes a new agent implementation in the specified repository root. */
      init: agents.init,
    }
  }

  get actions() {
    return {
      /** Runs an isolated, one-shot action agent for short-lived queries or tasks. */
      run: actions.runAgentAction,
    }
  }

  get loop() {
    return {
      /** Resolves the runtime configuration for a daemon-managed loop. */
      resolve: loops.resolveLoop,
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
      /** Finds the nearest valid repository root containing a `.git` or `.gitrepo` directory. */
      resolveRepositoryRoot: workforce.resolveRepositoryRoot,
      /** Discovers valid target agents that can be initialized within the workforce. */
      discoverInitCandidates: workforce.discoverWorkforceInitCandidates,
      /** Initializes a new agent workforce structure in the repository root. */
      initialize: workforce.initializeWorkforce,
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
