import { actionIpcRoutes } from "@goddard-ai/action/daemon-ipc"
import { agentIpcRoutes } from "@goddard-ai/agent/daemon-ipc"
import { authIpcRoutes } from "@goddard-ai/auth/daemon-ipc"
import { fileSearchIpcRoutes } from "@goddard-ai/file-search/daemon-ipc"
import { inboxIpcRoutes } from "@goddard-ai/inbox/daemon-ipc"
import { composeIpcRoutes } from "@goddard-ai/ipc"
import { loopIpcRoutes } from "@goddard-ai/loop/daemon-ipc"
import { pullRequestIpcRoutes } from "@goddard-ai/pull-request/daemon-ipc"
import { reviewSessionIpcRoutes } from "@goddard-ai/review-session/daemon-ipc"
import { coreDaemonIpcRoutes } from "@goddard-ai/schema/daemon-ipc"
import { sessionIpcRoutes } from "@goddard-ai/session/daemon-ipc"
import { workforceIpcRoutes } from "@goddard-ai/workforce/daemon-ipc"

const defaultDaemonFeatureIpcRoutes = [
  actionIpcRoutes,
  authIpcRoutes,
  fileSearchIpcRoutes,
  agentIpcRoutes,
  sessionIpcRoutes,
  inboxIpcRoutes,
  pullRequestIpcRoutes,
  reviewSessionIpcRoutes,
  loopIpcRoutes,
  workforceIpcRoutes,
] as const

/** IPC route tree for the default daemon product surface. */
export const daemonIpcRoutes = composeIpcRoutes([
  coreDaemonIpcRoutes,
  ...defaultDaemonFeatureIpcRoutes,
])
