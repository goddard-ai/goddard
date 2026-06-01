import { AsyncContext } from "@b9g/async-context"
import type { SessionId } from "@goddard-ai/session/schema"

/** Active loop runtime identity carried while one loop is executing work. */
export type LoopContext = {
  rootDir: string
  loopName: string
  sessionId: SessionId
  acpSessionId: string
}

export const LoopContext = new AsyncContext.Variable<LoopContext>()
