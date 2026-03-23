#!/usr/bin/env node
import * as acp from "@agentclientprotocol/sdk"
import * as os from "node:os"
import { createAgentMessageStream } from "../session/acp.ts"
import { spawnAgentProcess } from "../session/manager.ts"
import { ACPAdapterNames } from "@goddard-ai/schema/acp-adapters"

async function testAdapter(adapterName: string, prompt?: string) {
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

  console.log(`\n=== Session for ${adapterName} ===`)
  console.dir(session, { depth: null })

  if (prompt) {
    console.log(`\n--- Sending Prompt: "${prompt}" ---`)
    try {
      await connection.prompt({
        sessionId: session.sessionId,
        prompt: [{ type: "text", text: prompt }],
      })
      console.log(`\n--- Prompt Sent ---`)
    } catch (e) {
      console.log(`\n--- Failed to send prompt: ${e instanceof Error ? e.message : String(e)} ---`)
    }
  }

  processHandle.kill()
}

import { command, run, restPositionals, string, flag, option, optional } from "cmd-ts"

const app = command({
  name: "goddard-test-acp-session",
  args: {
    adapters: restPositionals({
      type: string,
      displayName: "adapter-name",
      description: "Name(s) of the adapters to test",
    }),
    all: flag({
      long: "all",
      description: "Test all available adapters",
    }),
    prompt: option({
      type: optional(string),
      long: "prompt",
      description: "Provide a one-shot prompt to send to the session",
    }),
  },
  handler: async ({ adapters, all, prompt }) => {
    let adaptersToTest: string[] = []

    if (all) {
      adaptersToTest = [...ACPAdapterNames]
    } else if (adapters.length > 0) {
      adaptersToTest = adapters
    } else {
      console.error("Usage: goddard-test-acp-session <adapter-name...> | --all")
      process.exit(1)
    }

    for (const adapterName of adaptersToTest) {
      await testAdapter(adapterName, prompt)
    }

    process.exit(0)
  },
})

run(app, process.argv.slice(2)).catch((error) => {
  console.error(error)
  process.exit(1)
})
