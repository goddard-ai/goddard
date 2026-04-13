import { InMemoryBackendControlPlane, startBackendServer } from "@goddard-ai/backend"
import { expect, test } from "bun:test"

import { BackendUnauthenticatedError, createBackendClient } from "../src/backend.ts"

test("daemon backend client creates PRs and checks managed status through rouzer route helpers", async () => {
  const controlPlane = new InMemoryBackendControlPlane()
  const server = await startBackendServer(controlPlane, { port: 0 })
  const baseUrl = `http://127.0.0.1:${server.port}`
  let authorization: string | null = null

  try {
    const flow = controlPlane.startDeviceFlow({ githubUsername: "alec" })
    const session = controlPlane.completeDeviceFlow({
      deviceCode: flow.deviceCode,
      githubUsername: "alec",
    })
    authorization = `Bearer ${session.token}`

    const client = createBackendClient({
      baseUrl,
      getAuthorizationHeader: () => authorization,
    })
    const pr = await client.pr.create({
      owner: "goddard-ai",
      repo: "sdk",
      title: "Add daemon backend route client",
      body: "Ship it",
      head: "feat/daemon-backend-routes",
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

test("daemon backend client uses injected auth state for authenticated requests", async () => {
  const controlPlane = new InMemoryBackendControlPlane()
  const server = await startBackendServer(controlPlane, { port: 0 })
  const baseUrl = `http://127.0.0.1:${server.port}`
  let authorization: string | null = null

  try {
    const client = createBackendClient({
      baseUrl,
      getAuthorizationHeader: () => authorization,
      clearAuthorization: () => {
        authorization = null
      },
    })
    const start = await client.auth.startDeviceFlow({ githubUsername: "alec" })
    const session = await client.auth.completeDeviceFlow({
      deviceCode: start.deviceCode,
      githubUsername: "alec",
    })

    authorization = `Bearer ${session.token}`
    await expect(client.auth.whoami()).resolves.toEqual(session)

    await client.auth.logout()
    expect(authorization).toBeNull()
  } finally {
    await server.close()
  }
})

test("daemon backend client subscribes to unified stream via rouzer route response", async () => {
  const controlPlane = new InMemoryBackendControlPlane()
  const server = await startBackendServer(controlPlane, { port: 0 })
  const baseUrl = `http://127.0.0.1:${server.port}`
  let authorization: string | null = null
  let subscription: Awaited<
    ReturnType<ReturnType<typeof createBackendClient>["stream"]["subscribe"]>
  > | null = null

  try {
    const flow = controlPlane.startDeviceFlow({ githubUsername: "alec" })
    const session = controlPlane.completeDeviceFlow({
      deviceCode: flow.deviceCode,
      githubUsername: "alec",
    })
    authorization = `Bearer ${session.token}`

    const client = createBackendClient({
      baseUrl,
      getAuthorizationHeader: () => authorization,
    })
    subscription = await client.stream.subscribe()

    const eventPromise = new Promise<unknown>((resolve) => {
      subscription!.on("event", resolve)
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
  } finally {
    subscription?.close()
    await Bun.sleep(10)
    await server.close()
  }
})

test("daemon backend client reports stream auth failures as unauthenticated errors", async () => {
  const controlPlane = new InMemoryBackendControlPlane()
  const server = await startBackendServer(controlPlane, { port: 0 })
  const baseUrl = `http://127.0.0.1:${server.port}`

  try {
    const client = createBackendClient({
      baseUrl,
      getAuthorizationHeader: () => "Bearer invalid-token",
    })

    await expect(client.stream.subscribe()).rejects.toBeInstanceOf(BackendUnauthenticatedError)
  } finally {
    await server.close()
  }
})
