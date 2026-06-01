import { AsyncContext } from "@b9g/async-context"
import type { DaemonSessionId } from "@goddard-ai/schema/id"

/** Active loop runtime identity carried while one loop is executing work. */
export type LoopContext = {
  rootDir: string
  loopName: string
  sessionId: DaemonSessionId
  acpSessionId: string
}

export const LoopContext = new AsyncContext.Variable<LoopContext>()
