import { $type, defineIpcRoutes, http } from "@goddard-ai/ipc"

/** Core daemon IPC routes that are not owned by feature packages. */
export const coreDaemonIpcRoutes = defineIpcRoutes({
  daemon: http.resource("daemon", {
    health: http.get("health", {
      response: $type<{ ok: boolean }>(),
    }),
  }),
})
