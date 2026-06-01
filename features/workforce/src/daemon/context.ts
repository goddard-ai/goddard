import { AsyncContext } from "@b9g/async-context"

/** Authenticated workforce actor identity attached to one mutation call. */
export type WorkforceActorContext = {
  sessionId: string | null
  rootDir: string | null
  agentId: string | null
  requestId: string | null
}

/** Active workforce dispatch metadata carried while one request attempt is running. */
export type WorkforceDispatchContext = {
  rootDir: string
  agentId: string
  requestId: string
  attempt: number
}

export const WorkforceActorContext = new AsyncContext.Variable<WorkforceActorContext>()
export const WorkforceDispatchContext = new AsyncContext.Variable<WorkforceDispatchContext>()
