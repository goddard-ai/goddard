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
    const flow = controlPlane.startDeviceFlow(githubStart("alec"))
    const session = controlPlane.completeDeviceFlow({
      deviceCode: flow.deviceCode,
      providerIdentity: githubIdentity("alec"),
    })
    authorization = `Bearer ${session.token}`

    const client = createBackendClient({
      baseUrl,
      getAuthorizationHeader: () => authorization,
    })
    const pr = await client.pullRequests.create({
      provider: "github",
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
        provider: "github",
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
    const start = await client.auth.device.start(githubStart("alec"))
    const session = await client.auth.device.complete({
      deviceCode: start.deviceCode,
      providerIdentity: githubIdentity("alec"),
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

  try {
    const flow = controlPlane.startDeviceFlow(githubStart("alec"))
    const session = controlPlane.completeDeviceFlow({
      deviceCode: flow.deviceCode,
      providerIdentity: githubIdentity("alec"),
    })
    authorization = `Bearer ${session.token}`

    const client = createBackendClient({
      baseUrl,
      getAuthorizationHeader: () => authorization,
    })
    const eventsPromise = client.events.stream({ names: ["pr.created"] })
    const eventPromise = eventsPromise.then(readFirstEvent)
    await Bun.sleep(10)

    const pr = await client.pullRequests.create({
      provider: "github",
      owner: "goddard-ai",
      repo: "sdk",
      title: "Stream me",
      body: "Done",
      head: "feat/stream",
      base: "main",
    })

    const event = await eventPromise
    expect(pr.number).toBe(1)
    expect(event.name).toBe(REMOTE_REPO_PULL_REQUEST_CREATED)
    expect(event.payload.type).toBe("pr.created")
    expect(event.payload.prNumber).toBe(1)
  } finally {
    await server.close()
  }
})

function githubStart(login: string) {
  return {
    provider: "github",
    loginHint: login,
  }
}

function githubIdentity(login: string) {
  return {
    provider: "github",
    subject: String(hashTestIdentity(login)),
    displayName: login,
  }
}

function hashTestIdentity(value: string): number {
  let hash = 0
  for (let i = 0; i < value.length; i += 1) {
    hash = (hash << 5) - hash + value.charCodeAt(i)
    hash |= 0
  }
  return Math.abs(hash) + 1000
}

test("daemon backend client reports missing stream auth as unauthenticated errors", async () => {
  const client = createBackendClient({
    baseUrl: "https://goddardai.org/api",
    fetchImpl: async () => new Response("unauthorized", { status: 401 }),
    getAuthorizationHeader: () => null,
  })

  await expect(client.events.stream({})).rejects.toBeInstanceOf(BackendUnauthenticatedError)
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

    await expect(client.events.stream({})).rejects.toBeInstanceOf(BackendUnauthenticatedError)
  } finally {
    await server.close()
  }
})

async function readFirstEvent<T>(events: AsyncIterable<T>): Promise<T> {
  for await (const event of events) {
    return event
  }

  throw new Error("Backend event stream ended before emitting data")
}
