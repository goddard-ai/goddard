import { actionIpcRoutes } from "@goddard-ai/action/daemon-ipc"
import { adapterIpcRoutes } from "@goddard-ai/adapter/daemon-ipc"
import { authIpcRoutes } from "@goddard-ai/auth/daemon-ipc"
import { inboxIpcRoutes } from "@goddard-ai/inbox/daemon-ipc"
import { $type, composeIpcRoutes, defineIpcRoutes, http } from "@goddard-ai/ipc"
import { loopIpcRoutes } from "@goddard-ai/loop/daemon-ipc"
import { pullRequestIpcRoutes } from "@goddard-ai/pull-request/daemon-ipc"
import { reviewSessionIpcRoutes } from "@goddard-ai/review-session/daemon-ipc"
import { sessionIpcRoutes } from "@goddard-ai/session/daemon-ipc"
import { workforceIpcRoutes } from "@goddard-ai/workforce/daemon-ipc"

const coreDaemonIpcRoutes = defineIpcRoutes({
  daemon: http.resource("daemon", {
    health: http.get("health", {
      response: $type<{ ok: boolean }>(),
    }),
  }),
})

/** IPC route tree shared by daemon clients and server composition roots. */
export const daemonIpcRoutes = composeIpcRoutes([
  coreDaemonIpcRoutes,
  actionIpcRoutes,
  adapterIpcRoutes,
  authIpcRoutes,
  inboxIpcRoutes,
  loopIpcRoutes,
  pullRequestIpcRoutes,
  reviewSessionIpcRoutes,
  sessionIpcRoutes,
  workforceIpcRoutes,
])
