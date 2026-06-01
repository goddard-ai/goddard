import { actionIpcRoutes } from "@goddard-ai/action/daemon-ipc"
import { adapterIpcRoutes } from "@goddard-ai/adapter/daemon-ipc"
import { authIpcRoutes } from "@goddard-ai/auth/daemon-ipc"
import { inboxIpcRoutes } from "@goddard-ai/inbox/daemon-ipc"
import { composeIpcRoutes } from "@goddard-ai/ipc"
import { loopIpcRoutes } from "@goddard-ai/loop/daemon-ipc"
import { pullRequestIpcRoutes } from "@goddard-ai/pull-request/daemon-ipc"
import { reviewSessionIpcRoutes } from "@goddard-ai/review-session/daemon-ipc"
import { coreDaemonIpcRoutes } from "@goddard-ai/schema/daemon-ipc"
import { sessionIpcRoutes } from "@goddard-ai/session/daemon-ipc"
import { workforceIpcRoutes } from "@goddard-ai/workforce/daemon-ipc"

import { selectDefaultFeatureContributions } from "./features.ts"

const defaultDaemonFeatureIpcRoutes = selectDefaultFeatureContributions({
  action: actionIpcRoutes,
  adapter: adapterIpcRoutes,
  auth: authIpcRoutes,
  session: sessionIpcRoutes,
  inbox: inboxIpcRoutes,
  pullRequest: pullRequestIpcRoutes,
  reviewSession: reviewSessionIpcRoutes,
  loop: loopIpcRoutes,
  workforce: workforceIpcRoutes,
})

/** IPC route tree for the default daemon product surface. */
export const daemonIpcRoutes = composeIpcRoutes([
  coreDaemonIpcRoutes,
  ...defaultDaemonFeatureIpcRoutes,
])
