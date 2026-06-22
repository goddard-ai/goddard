import { event } from "@goddard-ai/sdk-plugin"

import type { WorkforceEventEnvelope } from "./schema.ts"

export const workforceEvents = {
  "workforce.ledger.event": event<WorkforceEventEnvelope>({ debug: "workforce.stream" }),
}

export type WorkforceEventDefinitions = typeof workforceEvents
