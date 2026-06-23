import { InMemoryBackendControlPlane, startBackendServer } from "@goddard-ai/backend"
import { REMOTE_REPO_PULL_REQUEST_CREATED } from "@goddard-ai/remote-repo/backend"
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
    const pr = await client.pullRequests.create({
      owner: "goddard-ai",
      repo: "sdk",
      title: "Add daemon backend route client",
      body: "Ship it",
      head: "feat/daemon-backend-routes",
      base: "main",
    })

    expect(pr.number).toBe(1)
    await expect(
      client.pullRequests.managed({
        owner: "goddard-ai",
        repo: "sdk",
        prNumber: pr.number,
      }),
    ).resolves.toEqual({ managed: true })
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
    })
    const start = await client.auth.device.start({ githubUsername: "alec" })
    const session = await client.auth.device.complete({
      deviceCode: start.deviceCode,
      githubUsername: "alec",
    })

    authorization = `Bearer ${session.token}`
    await expect(client.auth.session.current()).resolves.toEqual(session)
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

    const pr = await client.pullRequests.create({
      owner: "goddard-ai",
      repo: "sdk",
      title: "Stream me",
      body: "Done",
      head: "feat/stream",
      base: "main",
    })

    const event = (await eventPromise) as {
      name: string
      payload: { type: string; prNumber: number }
    }
    expect(pr.number).toBe(1)
    expect(event.name).toBe(REMOTE_REPO_PULL_REQUEST_CREATED)
    expect(event.payload.type).toBe("pr.created")
    expect(event.payload.prNumber).toBe(1)
  } finally {
    subscription?.close()
    await Bun.sleep(10)
    await server.close()
  }
})

test("daemon backend client reports missing stream auth as unauthenticated errors", async () => {
  const client = createBackendClient({
    baseUrl: "https://goddardai.org/api",
    getAuthorizationHeader: () => null,
  })

  await expect(client.stream.subscribe()).rejects.toBeInstanceOf(BackendUnauthenticatedError)
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
