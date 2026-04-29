import { unstable_dev } from "wrangler"

const [, , backendDir, portArg] = process.argv
process.chdir(backendDir)

let worker
let stopping = false

try {
  worker = await unstable_dev("src/worker.ts", {
    config: "wrangler.toml",
    experimental: { disableExperimentalWarning: true },
    localProtocol: "http",
    persist: false,
    port: Number(portArg),
    vars: {
      GODDARD_BACKEND_TEST_MODE: "1",
      TURSO_DB_AUTH_TOKEN: "test-token",
      TURSO_DB_URL: "libsql://test",
    },
  })

  console.log(`__GODDARD_WRANGLER_READY__${JSON.stringify({ port: worker.port })}`)
} catch (error) {
  console.error(error)
  process.exit(1)
}

async function stop() {
  if (stopping) {
    return
  }

  stopping = true
  await worker?.stop()
  process.exit(0)
}

process.stdin.resume()
process.stdin.on("data", () => {
  void stop()
})
process.stdin.on("end", () => {
  void stop()
})
process.on("SIGINT", () => {
  void stop()
})
process.on("SIGTERM", () => {
  void stop()
})
