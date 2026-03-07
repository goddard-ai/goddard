import alchemy from "alchemy"
import { Worker, DurableObjectNamespace } from "alchemy/cloudflare"

const app = await alchemy("goddard-backend")

const repoStream = DurableObjectNamespace("REPO_STREAM", {
  className: "RepoStream",
})

export const worker = await Worker("api", {
  entrypoint: "./src/worker.ts",
  url: true,
  bindings: {
    REPO_STREAM: repoStream,
  },
})

console.log({ url: worker.url })

await app.finalize()
