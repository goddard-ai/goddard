import alchemy from "alchemy"
import { DurableObjectNamespace, Worker } from "alchemy/cloudflare"

const app = await alchemy("goddard-backend")

const userStream = DurableObjectNamespace("USER_STREAM", {
  className: "UserStream",
})

const cloudSession = DurableObjectNamespace("CLOUD_SESSION", {
  className: "CloudSession",
})

export const worker = await Worker("api", {
  entrypoint: "./src/worker.ts",
  url: true,
  bindings: {
    USER_STREAM: userStream,
    CLOUD_SESSION: cloudSession,
  },
})

console.log({ url: worker.url })

await app.finalize()
