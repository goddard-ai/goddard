import { $type, route } from "rouzer"
import type { DaemonHealth } from "../../daemon.ts"

/** Reports daemon liveness and basic health state. */
export const healthRoute = route("health", {
  GET: {
    response: $type<DaemonHealth>(),
  },
})
