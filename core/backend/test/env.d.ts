import type * as worker from "../src/worker.ts"

declare global {
  namespace Cloudflare {
    interface Env {
      TURSO_DB_URL: string
      TURSO_DB_AUTH_TOKEN: string
      USER_STREAM: DurableObjectNamespace
      CLOUD_SESSION: DurableObjectNamespace
    }

    interface GlobalProps {
      mainModule: typeof worker
      durableNamespaces: "CloudSession" | "UserStream"
    }
  }
}
