import alchemy from "alchemy"
import { Worker, DurableObjectNamespace } from "alchemy/cloudflare"

const app = await alchemy("goddard-backend")

const userStream = DurableObjectNamespace("USER_STREAM", {
  className: "UserStream",
})

export const worker = await Worker("api", {
  entrypoint: "./src/worker.ts",
  url: true,
  bindings: {
    USER_STREAM: userStream,
  },
})

console.log({ url: worker.url })

await app.finalize()
