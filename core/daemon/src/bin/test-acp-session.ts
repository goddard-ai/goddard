#!/usr/bin/env node
import * as acp from "@agentclientprotocol/sdk"
import * as os from "node:os"
import { createAgentMessageStream } from "../session/acp.ts"
import { spawnAgentProcess } from "../session/manager.ts"
import { ACPAdapterNames } from "@goddard-ai/schema/acp-adapters"

async function testAdapter(adapterName: string) {
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

  processHandle.kill()
}

import { command, run, restPositionals, string, flag } from "cmd-ts"

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
  },
  handler: async ({ adapters, all }) => {
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
      await testAdapter(adapterName)
    }

    process.exit(0)
  },
})

run(app, process.argv.slice(2)).catch((error) => {
  console.error(error)
  process.exit(1)
})
