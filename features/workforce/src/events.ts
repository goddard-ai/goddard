import { event, type EventDefinition } from "@goddard-ai/sdk-plugin"

import type { WorkforceEventEnvelope } from "./schema.ts"

export type WorkforceEventDefinitions = {
  "workforce.ledger.event": EventDefinition<WorkforceEventEnvelope>
}

export const workforceEvents = {
  "workforce.ledger.event": event<WorkforceEventEnvelope>({ debug: "workforce.stream" }),
} satisfies WorkforceEventDefinitions
