#!/usr/bin/env node
import * as acp from "@agentclientprotocol/sdk"
import * as os from "node:os"
import { createAgentMessageStream } from "../session/acp.ts"
import { spawnAgentProcess } from "../session/manager.ts"

async function main() {
  const adapterName = process.argv[2]
  if (!adapterName) {
    console.error("Usage: goddard-test-acp-session <adapter-name>")
    process.exit(1)
  }

  const processHandle = await spawnAgentProcess("http://localhost:0", "test-token", {
    agent: adapterName,
    cwd: process.cwd(),
    agentBinDir: os.tmpdir(),
  })

  const stream = createAgentMessageStream(processHandle.stdin, processHandle.stdout)

  const connection = new acp.ClientSideConnection(
    () => ({
      async requestPermission() {
        return { outcome: { outcome: "cancelled" } }
      },
      async sessionUpdate() {},
    }),
    stream,
  )

  await connection.initialize({
    protocolVersion: acp.PROTOCOL_VERSION,
    clientInfo: { name: "test", version: "1.0.0" },
  })

  const session = await connection.newSession({
    cwd: process.cwd(),
    mcpServers: [],
  })

  console.dir(session, { depth: null })

  processHandle.kill()
  process.exit(0)
}

main().catch((error) => {
  console.error(error)
  process.exit(1)
})
