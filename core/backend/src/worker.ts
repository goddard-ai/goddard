import adapter from "@hattip/adapter-cloudflare-workers/no-static"
import type { Env } from "./env.ts"
import { createBackendRouter } from "./api/router.ts"

const router = createBackendRouter({
  broadcastEvent: async (_env, _event) => {
    // TODO: implement real-time fanout via Cloudflare Queues or similar.
    // Each Worker invocation is isolated; without a shared-state layer
    // (Queues, KV pub/sub) broadcast to a connected SSE client in another
    // invocation is not possible in pure Workers.
  },
  handleUserStream: async (_env, _githubUsername, request) => {
    return createSseStream(request)
  },
})

export default {
  fetch: adapter(router),
} satisfies ExportedHandler<Env>

function createSseStream(_request: Request): Response {
  const encoder = new TextEncoder()

  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      controller.enqueue(encoder.encode(": connected\n\n"))
    },
    cancel() {},
  })

  return new Response(stream, {
    status: 200,
    headers: {
      "content-type": "text/event-stream",
      "cache-control": "no-cache, no-transform",
      connection: "keep-alive",
    },
  })
}
