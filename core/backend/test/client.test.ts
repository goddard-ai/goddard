import { test } from "vitest"
import * as assert from "node:assert/strict"
import { InMemoryTokenStorage } from "@goddard-ai/storage"
import { createBackendClient } from "../src/client.ts"
import { InMemoryBackendControlPlane, startBackendServer } from "../src/index.ts"

test("backend client creates PRs and checks managed status through rouzer route helpers", async () => {
  const controlPlane = new InMemoryBackendControlPlane()
  const server = await startBackendServer(controlPlane, { port: 0 })
  const baseUrl = `http://127.0.0.1:${server.port}`
  const tokenStorage = new InMemoryTokenStorage()

  try {
    const flow = controlPlane.startDeviceFlow({ githubUsername: "alec" })
    const session = controlPlane.completeDeviceFlow({
      deviceCode: flow.deviceCode,
      githubUsername: "alec",
    })
    await tokenStorage.setToken(session.token)

    const client = createBackendClient({ baseUrl, tokenStorage })
    const pr = await client.pr.create({
      owner: "goddard-ai",
      repo: "sdk",
      title: "Add backend client",
      body: "Ship it",
      head: "feat/backend-client",
      base: "main",
    })

    assert.equal(pr.number, 1)
    assert.equal(
      await client.pr.isManaged({ owner: "goddard-ai", repo: "sdk", prNumber: pr.number }),
      true,
    )
  } finally {
    await server.close()
  }
})

test("backend client subscribes to repo stream via rouzer route response", async () => {
  const controlPlane = new InMemoryBackendControlPlane()
  const server = await startBackendServer(controlPlane, { port: 0 })
  const baseUrl = `http://127.0.0.1:${server.port}`
  const tokenStorage = new InMemoryTokenStorage()

  try {
    const flow = controlPlane.startDeviceFlow({ githubUsername: "alec" })
    const session = controlPlane.completeDeviceFlow({
      deviceCode: flow.deviceCode,
      githubUsername: "alec",
    })
    await tokenStorage.setToken(session.token)

    const client = createBackendClient({ baseUrl, tokenStorage })
    const subscription = await client.stream.subscribeToRepo({ owner: "goddard-ai", repo: "sdk" })

    const eventPromise = new Promise<unknown>((resolve) => {
      subscription.on("event", resolve)
    })

    const pr = await client.pr.create({
      owner: "goddard-ai",
      repo: "sdk",
      title: "Stream me",
      body: "Done",
      head: "feat/stream",
      base: "main",
    })

    const event = (await eventPromise) as { type: string; prNumber: number }
    assert.equal(pr.number, 1)
    assert.equal(event.type, "pr.created")
    assert.equal(event.prNumber, 1)

    subscription.close()
  } finally {
    await server.close()
  }
})
