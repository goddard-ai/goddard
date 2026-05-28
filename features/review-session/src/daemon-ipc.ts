import { $type, defineIpcRoutes, http } from "@goddard-ai/ipc"

import {
  GetReviewSessionRequest,
  MountReviewSessionRequest,
  RunReviewSessionRequest,
  UnmountReviewSessionRequest,
  type ReviewSessionResponse,
} from "./schema.ts"

export const reviewSessionIpcRoutes = defineIpcRoutes({
  reviewSession: http.resource("review-session", {
    /** Reads review-session state for one daemon-managed session worktree. */
    get: http.post("get", {
      body: GetReviewSessionRequest,
      response: $type<ReviewSessionResponse>(),
    }),
    /** Mounts a review session for one daemon-managed session worktree. */
    mount: http.post("mount", {
      body: MountReviewSessionRequest,
      response: $type<ReviewSessionResponse>(),
    }),
    /** Runs one mounted review session immediately. */
    run: http.post("run", {
      body: RunReviewSessionRequest,
      response: $type<ReviewSessionResponse>(),
    }),
    /** Unmounts a review session from one daemon-managed session worktree. */
    unmount: http.post("unmount", {
      body: UnmountReviewSessionRequest,
      response: $type<ReviewSessionResponse>(),
    }),
  }),
})
