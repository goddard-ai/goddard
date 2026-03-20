import { InMemoryTokenStorage } from "@goddard-ai/storage"
import { expect, test } from "vitest"
import { InMemoryBackendControlPlane, startBackendServer } from "../../src/index.ts"
import { createBackendClient } from "../src/index.ts"

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

    expect(pr.number).toBe(1)
    await expect(
      client.pr.isManaged({ owner: "goddard-ai", repo: "sdk", prNumber: pr.number }),
    ).resolves.toBe(true)
  } finally {
    await server.close()
  }
})

test("backend client manages auth session state through token storage", async () => {
  const controlPlane = new InMemoryBackendControlPlane()
  const server = await startBackendServer(controlPlane, { port: 0 })
  const baseUrl = `http://127.0.0.1:${server.port}`
  const tokenStorage = new InMemoryTokenStorage()

  try {
    const client = createBackendClient({ baseUrl, tokenStorage })
    const start = await client.auth.startDeviceFlow({ githubUsername: "alec" })
    const session = await client.auth.completeDeviceFlow({
      deviceCode: start.deviceCode,
      githubUsername: "alec",
    })

    await expect(tokenStorage.getToken()).resolves.toBe(session.token)
    await expect(client.auth.whoami()).resolves.toEqual(session)

    await client.auth.logout()
    await expect(tokenStorage.getToken()).resolves.toBeNull()
  } finally {
    await server.close()
  }
})

test("backend client subscribes to unified stream via rouzer route response", async () => {
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
    const subscription = await client.stream.subscribe()

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
    expect(pr.number).toBe(1)
    expect(event.type).toBe("pr.created")
    expect(event.prNumber).toBe(1)

    subscription.close()
  } finally {
    await server.close()
  }
})
