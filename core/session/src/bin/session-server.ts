#!/usr/bin/env node
import type { SessionParams, SessionServerLog } from "@goddard-ai/schema/session-server"
import { randomUUID } from "node:crypto"
import { serveAgent } from "../server.js"

function log(message: SessionServerLog) {
  process.stdout.write(JSON.stringify(message) + "\n")
}

export async function main(argv: string[]) {
  const agentParams = JSON.parse(argv[0]) as SessionParams
  const serverId = randomUUID()
  const startedServer = await serveAgent(serverId, agentParams)
  log({
    success: true,
    serverAddress: startedServer.serverAddress.href,
    serverId,
    sessionId: startedServer.sessionId,
  })
}

if (import.meta.main) {
  main(process.argv.slice(2)).catch((error) => {
    log({
      success: false,
      error: error instanceof Error ? error.message : String(error),
    })
    process.exit(1)
  })
}
