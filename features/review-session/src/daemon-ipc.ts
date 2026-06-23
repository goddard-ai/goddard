import { $type, defineIpcRoutes, http, ipcMetadata } from "@goddard-ai/ipc"

import {
  GetReviewSessionRequest,
  MountReviewSessionRequest,
  RunReviewSessionRequest,
  UnmountReviewSessionRequest,
  type ReviewSessionResponse,
} from "./schema.ts"

export const reviewSessionIpcRoutes = defineIpcRoutes({
  reviewSession: http.resource("review-session", {
    ...ipcMetadata({
      description: "Review-session state and lifecycle control.",
    }),
    /** Reads review-session state for one session worktree. */
    get: http.post("get", {
      ...ipcMetadata({
        description: "Reads review-session state for one session worktree.",
      }),
      body: GetReviewSessionRequest,
      response: $type<ReviewSessionResponse>(),
    }),
    /** Mounts a review session for one session worktree. */
    mount: http.post("mount", {
      ...ipcMetadata({
        description: "Mounts a review session for one session worktree.",
      }),
      body: MountReviewSessionRequest,
      response: $type<ReviewSessionResponse>(),
    }),
    /** Runs one mounted review session immediately. */
    run: http.post("run", {
      ...ipcMetadata({
        description: "Runs one mounted review session immediately.",
      }),
      body: RunReviewSessionRequest,
      response: $type<ReviewSessionResponse>(),
    }),
    /** Unmounts a review session from one session worktree. */
    unmount: http.post("unmount", {
      ...ipcMetadata({
        description: "Unmounts a review session from one session worktree.",
      }),
      body: UnmountReviewSessionRequest,
      response: $type<ReviewSessionResponse>(),
    }),
  }),
})
