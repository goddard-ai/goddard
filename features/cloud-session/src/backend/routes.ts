import { BearerHeaders } from "@goddard-ai/auth/schema"
import { $type, defineBackendRoutes, http } from "@goddard-ai/backend-plugin"
import { z } from "zod"

import {
  CloudSessionCommand,
  CloudSessionId,
  CreateCloudSessionRequest,
  type CloudSessionCommandResponse,
  type CloudSessionSyncResponse,
  type CreateCloudSessionResponse,
} from "../schema.ts"

const CloudSessionPath = z.object({
  sessionId: CloudSessionId,
})

const CloudSessionSyncQuery = z.object({
  after: z.coerce.number().int().nonnegative().optional(),
  limit: z.coerce.number().int().positive().optional(),
})

/** Cloud-session-owned backend routes for lifecycle and coordinator synchronization. */
export const cloudSessionBackendRoutes = defineBackendRoutes({
  cloudSessionCreateRoute: http.post("cloud/sessions", {
    headers: BearerHeaders,
    body: CreateCloudSessionRequest,
    response: $type<CreateCloudSessionResponse>(),
  }),
  cloudSessionCreateByIdRoute: http.post("cloud/sessions/:sessionId", {
    path: CloudSessionPath,
    headers: BearerHeaders,
    body: CreateCloudSessionRequest,
    response: $type<CreateCloudSessionResponse>(),
  }),
  cloudSessionSyncRoute: http.get("cloud/sessions/:sessionId/sync", {
    path: CloudSessionPath,
    query: CloudSessionSyncQuery,
    headers: BearerHeaders,
    response: $type<CloudSessionSyncResponse>(),
  }),
  cloudSessionCommandRoute: http.post("cloud/sessions/:sessionId/commands", {
    path: CloudSessionPath,
    headers: BearerHeaders,
    body: CloudSessionCommand,
    response: $type<CloudSessionCommandResponse>(),
  }),
})
