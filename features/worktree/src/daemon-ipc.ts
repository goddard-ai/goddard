import { $type, defineIpcSchema } from "@goddard-ai/ipc"

import {
  GetSessionWorktreeRequest,
  MountSessionReviewSessionRequest,
  RunSessionReviewSessionRequest,
  UnmountSessionReviewSessionRequest,
  type GetSessionWorktreeResponse,
  type MutateSessionReviewSessionResponse,
} from "./schema.ts"

export const worktreeIpcSchema = defineIpcSchema({
  requests: {
    "session.worktree.get": {
      payload: GetSessionWorktreeRequest,
      response: $type<GetSessionWorktreeResponse>(),
    },
    "session.reviewSession.mount": {
      payload: MountSessionReviewSessionRequest,
      response: $type<MutateSessionReviewSessionResponse>(),
    },
    "session.reviewSession.run": {
      payload: RunSessionReviewSessionRequest,
      response: $type<MutateSessionReviewSessionResponse>(),
    },
    "session.reviewSession.unmount": {
      payload: UnmountSessionReviewSessionRequest,
      response: $type<MutateSessionReviewSessionResponse>(),
    },
  },
  streams: {},
})
