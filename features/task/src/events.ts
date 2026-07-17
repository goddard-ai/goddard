import { event } from "@goddard-ai/sdk-plugin"

import type { TaskChangedEvent } from "./schema.ts"

export const taskEvents = {
  "task.changed": event<TaskChangedEvent>({ debug: "task.stream" }),
}
