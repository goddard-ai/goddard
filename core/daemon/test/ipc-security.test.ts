import { spawnSync } from "node:child_process"
import { mkdir, mkdtemp, writeFile } from "node:fs/promises"
import { connect } from "node:net"
import { tmpdir } from "node:os"
import { dirname, join } from "node:path"
import { createBrowserDaemonIpcClient } from "@goddard-ai/daemon-client/browser"
import { createDaemonIpcClient } from "@goddard-ai/daemon-client/node"
import { createLogStore } from "@goddard-ai/logs"
import { getGlobalConfigPath } from "@goddard-ai/paths/node"
import type { DaemonPullRequest } from "@goddard-ai/pull-request/schema"
import type { DaemonSession } from "@goddard-ai/session/schema"
import { afterAll, afterEach, expect, test } from "bun:test"

import type { BackendClient } from "../src/backend.ts"
import { startDaemonServer, type DaemonServer } from "../src/ipc.ts"
import { configureLogging } from "../src/logging.ts"
import { send, subscribe } from "./ipc-client-helpers.ts"
import { resetComposedDaemonStore, type ComposedDaemonStore } from "./support/store.ts"
import { removeTemporaryPath } from "./support/temp.ts"

const cleanup: Array<() => Promise<void>> = []
const originalHome = process.env.HOME
const hostedOrigin = "https://app.goddardai.org"
const otherHostedOrigin = "https://other.goddardai.org"
const desktopWebviewOrigin = "http://desktop.goddard.local"
let sharedHomeDir: string | null = null
let db: ComposedDaemonStore = resetComposedDaemonStore({ filename: ":memory:" })

afterEach(async () => {
  while (cleanup.length > 0) {
    await cleanup.pop()?.()
  }
  db.close()
  db = resetComposedDaemonStore({ filename: ":memory:" })

  if (sharedHomeDir) {
    await removeTemporaryPath(sharedHomeDir)
    sharedHomeDir = null
  }

  if (originalHome === undefined) {
    delete process.env.HOME
  } else {
    process.env.HOME = originalHome
  }
})

afterAll(async () => {
  // Per-test cleanup above already restores HOME and removes shared temp directories.
})

test("daemon submit request requires a valid session token", async () => {
  const daemon = await startServer()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })

  const { logs } = await captureLogs(async () => {
    await expect(
      send(client, "pr.submit", {
        token: "",
        cwd: process.cwd(),
        title: "Ship daemon security",
        body: "Done.",
      }),
    ).rejects.toThrow(/invalid session token/i)
  })

  const received = logs.find((entry) => entry.event === "ipc.request_received")
  const failed = logs.find((entry) => entry.event === "ipc.request_failed")
  const receivedIpcRequest = received?.ipcRequest as Record<string, unknown> | undefined
  const failedIpcRequest = failed?.ipcRequest as Record<string, unknown> | undefined
  expect(received?.requestName).toBe("pr.submit")
  expect(received?.payload).toEqual({
    token: "[redacted]",
    cwd: process.cwd(),
    title: "Ship daemon security",
    body: "Done.",
  })
  expect(receivedIpcRequest?.opId).toBe(failedIpcRequest?.opId)
  expect(failed?.requestName).toBe("pr.submit")
})

test("daemon browser access is unavailable until explicitly enabled", async () => {
  const daemon = await startServer()

  const response = await browserFetch(daemon, "daemon/browser-access/pairing/start", {
    method: "POST",
    origin: hostedOrigin,
    body: {},
  })

  expect(response.status).toBe(403)
  expect(await response.json()).toEqual({ error: "Forbidden" })
})

test("daemon browser access preflight and origin validation fail closed", async () => {
  await useTempHome()
  await writeGlobalRootConfig({
    daemon: {
      browserAccess: {
        enabled: true,
        allowedOrigins: [hostedOrigin],
        desktopWebviewOrigins: [desktopWebviewOrigin],
      },
    },
  })
  const daemon = await startServer({ useExistingHome: true })

  const preflight = await browserFetch(daemon, "daemon/health", {
    method: "OPTIONS",
    origin: hostedOrigin,
    privateNetwork: true,
  })
  expect(preflight.status).toBe(204)
  expect(preflight.headers.get("access-control-allow-origin")).toBe(hostedOrigin)
  expect(preflight.headers.get("access-control-allow-private-network")).toBe("true")

  const missingOriginPreflight = await browserFetch(daemon, "daemon/health", {
    method: "OPTIONS",
  })
  expect(missingOriginPreflight.status).toBe(403)
  expect(missingOriginPreflight.headers.get("access-control-allow-origin")).toBeNull()

  for (const origin of ["null", "not an origin", "*", "https://evil.example"]) {
    const response = await browserFetch(daemon, "daemon/health", {
      method: "GET",
      origin,
    })
    expect(response.status).toBe(403)
    expect(response.headers.get("access-control-allow-origin")).toBeNull()
  }
})

test("daemon browser pairing issues origin-bound revocable tokens", async () => {
  await useTempHome()
  await writeGlobalRootConfig({
    daemon: {
      browserAccess: {
        enabled: true,
        allowedOrigins: [hostedOrigin, otherHostedOrigin],
        desktopWebviewOrigins: [desktopWebviewOrigin],
      },
    },
  })
  const daemon = await startServer({ useExistingHome: true })
  const localClient = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })

  const health = await browserFetch(daemon, "daemon/health", {
    method: "GET",
    origin: hostedOrigin,
  })
  expect(health.status).toBe(200)
  expect(await health.json()).toEqual({ ok: true })

  const unauthenticatedWhoami = await browserFetch(daemon, "auth/whoami", {
    method: "GET",
    origin: hostedOrigin,
  })
  expect(unauthenticatedWhoami.status).toBe(403)

  const started = await browserFetchJson<{
    pairingId: string
    code: string
    expiresAt: string
  }>(daemon, "daemon/browser-access/pairing/start", {
    method: "POST",
    origin: hostedOrigin,
    body: { label: "Browser" },
  })
  expect(started.pairingId).toStartWith("bap_")
  expect(started.code).toMatch(/^\d{6}$/)
  expect(new Date(started.expiresAt).getTime()).toBeGreaterThan(Date.now())

  const unconfirmedComplete = await browserFetch(daemon, "daemon/browser-access/pairing/complete", {
    method: "POST",
    origin: hostedOrigin,
    body: { pairingId: started.pairingId },
  })
  expect(unconfirmedComplete.status).toBe(400)

  await expect(
    localClient.daemon.browserAccess.pairing.confirm({
      pairingId: started.pairingId,
      code: started.code,
    }),
  ).resolves.toEqual({
    pairingId: started.pairingId,
    confirmed: true,
  })

  const completed = await browserFetchJson<{
    token: string
    clientId: string
    origin: string
  }>(daemon, "daemon/browser-access/pairing/complete", {
    method: "POST",
    origin: hostedOrigin,
    body: { pairingId: started.pairingId },
  })
  expect(completed.origin).toBe(hostedOrigin)
  expect(completed.clientId).toStartWith("bac_")
  expect(completed.token).toStartWith(`${completed.clientId}.`)

  const browserAccessState = db.metadata.get("browserAccess")
  expect(browserAccessState?.browserTokens[completed.clientId]).toMatchObject({
    origin: hostedOrigin,
    label: "Browser",
    revokedAt: null,
  })
  expect(JSON.stringify(browserAccessState?.browserTokens[completed.clientId])).not.toContain(
    completed.token,
  )

  const whoami = await browserFetchJson<{ githubUsername: string }>(daemon, "auth/whoami", {
    method: "GET",
    origin: hostedOrigin,
    token: completed.token,
  })
  expect(whoami.githubUsername).toBe("alec")

  const browserClient = createBrowserDaemonIpcClient({
    daemonUrl: daemon.daemonUrl,
    token: () => completed.token,
    fetch: createOriginFetch(hostedOrigin),
  })
  await expect(browserClient.auth.whoami()).resolves.toMatchObject({
    githubUsername: "alec",
  })

  seedAuthorizedSession({
    sessionId: "ses_browser_direct",
    token: "tok_browser_direct",
    owner: "trusted",
    repo: "widgets",
    allowedPrNumbers: [],
  })
  const browserStreamStatuses: string[] = []
  const abortController = new AbortController()
  const browserStream = await browserClient.inbox.streamItems(undefined, {
    signal: abortController.signal,
  })
  const browserStreamDone = (async () => {
    try {
      for await (const item of browserStream) {
        if (item.entityId !== "ses_browser_direct") {
          continue
        }
        browserStreamStatuses.push(item.status)
        if (browserStreamStatuses.length >= 2) {
          abortController.abort()
        }
      }
    } catch (error) {
      if (!abortController.signal.aborted) {
        throw error
      }
    }
  })()
  cleanup.push(async () => {
    abortController.abort()
    await browserStreamDone.catch(() => {})
  })

  await send(localClient, "session.reportTurnEnded", {
    id: "ses_browser_direct",
    scope: "Browser client",
    headline: "Direct stream update",
  })
  await send(localClient, "inbox.update", {
    entityId: "ses_browser_direct",
    status: "read",
  })
  await waitFor(async () => browserStreamStatuses.length >= 2)
  expect(browserStreamStatuses).toEqual(["unread", "read"])

  const replayed = await browserFetch(daemon, "auth/whoami", {
    method: "GET",
    origin: otherHostedOrigin,
    token: completed.token,
  })
  expect(replayed.status).toBe(403)

  await expect(
    localClient.daemon.browserAccess.client.revoke({ clientId: completed.clientId }),
  ).resolves.toEqual({ revoked: true })

  const revoked = await browserFetch(daemon, "auth/whoami", {
    method: "GET",
    origin: hostedOrigin,
    token: completed.token,
  })
  expect(revoked.status).toBe(403)
})

test("daemon desktop webview tokens are host-bootstrapped and origin-checked", async () => {
  await useTempHome()
  await writeGlobalRootConfig({
    daemon: {
      browserAccess: {
        enabled: true,
        allowedOrigins: [hostedOrigin],
        desktopWebviewOrigins: [desktopWebviewOrigin],
      },
    },
  })
  const daemon = await startServer({ useExistingHome: true })
  const localClient = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })

  const browserCreate = await browserFetch(daemon, "daemon/browser-access/webview-token/create", {
    method: "POST",
    origin: desktopWebviewOrigin,
    body: { origin: desktopWebviewOrigin },
  })
  expect(browserCreate.status).toBe(403)

  const webviewToken = await localClient.daemon.browserAccess.webviewToken.create({
    origin: desktopWebviewOrigin,
  })
  expect(webviewToken.origin).toBe(desktopWebviewOrigin)
  expect(new Date(webviewToken.expiresAt).getTime()).toBeGreaterThan(Date.now())

  const whoami = await browserFetchJson<{ githubUsername: string }>(daemon, "auth/whoami", {
    method: "GET",
    origin: desktopWebviewOrigin,
    token: webviewToken.token,
  })
  expect(whoami.githubUsername).toBe("alec")

  const wrongOrigin = await browserFetch(daemon, "auth/whoami", {
    method: "GET",
    origin: hostedOrigin,
    token: webviewToken.token,
  })
  expect(wrongOrigin.status).toBe(403)
})

test("daemon hides unexpected handler crashes from IPC clients", async () => {
  await useTempHome()
  const repoDir = await createGitRepoFixture({
    owner: "trusted",
    repo: "widgets",
    branch: "feature/secure-daemon",
  })

  const daemon = await startServer({
    sdk: {
      pr: {
        create: async () => {
          throw new Error("github exploded")
        },
        reply: async () => ({ success: true }),
      },
    },
    useExistingHome: true,
  })
  seedAuthorizedSession({
    sessionId: "ses_crash",
    token: "tok_session",
    owner: "trusted",
    repo: "widgets",
    allowedPrNumbers: [],
  })

  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const { logs } = await captureLogs(async () => {
    await expect(
      send(client, "pr.submit", {
        token: "tok_session",
        cwd: repoDir,
        title: "Ship daemon security",
        body: "Done.",
      }),
    ).rejects.toThrow(/internal server error/i)
  })

  const failed = logs.find((entry) => entry.event === "ipc.request_failed")
  expect(failed?.requestName).toBe("pr.submit")
  expect(failed?.errorMessage).toBe("github exploded")
})

test("daemon submit request enforces trusted repo context and records created PR access", async () => {
  await useTempHome()
  const repoDir = await createGitRepoFixture({
    owner: "evil",
    repo: "fork",
    branch: "feature/secure-daemon",
  })

  const createCalls: Array<Record<string, unknown>> = []

  const daemon = await startServer({
    sdk: {
      auth: {
        device: {
          start: async () => ({
            deviceCode: "dev_1",
            userCode: "ABCD-1234",
            verificationUri: "https://github.com/login/device",
            expiresIn: 900,
            interval: 5,
          }),
          complete: async () => ({
            token: "tok_1",
            githubUsername: "alec",
            githubUserId: 42,
          }),
        },
        whoami: async () => ({
          token: "tok_1",
          githubUsername: "alec",
          githubUserId: 42,
        }),
      },
      pr: {
        create: async (input) => {
          createCalls.push(input)
          return {
            number: 42,
            url: "https://github.com/trusted/widgets/pull/42",
          }
        },
        reply: async () => ({ success: true }),
      },
    },
    useExistingHome: true,
  })
  seedAuthorizedSession({
    sessionId: "ses_42",
    token: "tok_session",
    owner: "trusted",
    repo: "widgets",
    allowedPrNumbers: [],
  })

  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const { logs } = await captureLogs(async () => {
    await send(client, "pr.submit", {
      token: "tok_session",
      cwd: repoDir,
      title: "Ship daemon security",
      body: "Done.",
    })
  })

  expect(createCalls).toEqual([
    {
      owner: "trusted",
      repo: "widgets",
      title: "Ship daemon security",
      body: "Done.",
      head: "feature/secure-daemon",
      base: "main",
    },
  ])
  expect(db.sessions.get("ses_42")?.permissions?.allowedPrNumbers).toEqual([42])
  expect(
    db.pullRequests.findMany().map(({ host, owner, repo, prNumber, cwd }: DaemonPullRequest) => ({
      host,
      owner,
      repo,
      prNumber,
      cwd,
    })),
  ).toEqual([
    {
      host: "github",
      owner: "trusted",
      repo: "widgets",
      prNumber: 42,
      cwd: repoDir,
    },
  ])
  const pullRequest = db.pullRequests.first({
    where: {
      host: "github",
      owner: "trusted",
      repo: "widgets",
      prNumber: 42,
    },
  })
  expect(db.inboxItems.first({ where: { entityId: pullRequest!.id } })).toMatchObject({
    entityId: pullRequest!.id,
    reason: "pull_request.created",
    status: "unread",
    priority: "normal",
    scope: "Session",
    headline: "Ship daemon security",
  })
  await expect(send(client, "pr.get", { id: pullRequest!.id })).resolves.toMatchObject({
    pullRequest: {
      id: pullRequest!.id,
      owner: "trusted",
      repo: "widgets",
      prNumber: 42,
    },
  })

  const received = logs.find((entry) => entry.event === "ipc.request_received")
  const responded = logs.find((entry) => entry.event === "ipc.response_sent")
  const receivedIpcRequest = received?.ipcRequest as Record<string, unknown> | undefined
  const respondedIpcRequest = responded?.ipcRequest as Record<string, unknown> | undefined
  expect(received?.requestName).toBe("pr.submit")
  expect(responded?.requestName).toBe("pr.submit")
  expect(receivedIpcRequest?.opId).toBe(respondedIpcRequest?.opId)
  expect(receivedIpcRequest?.sessionId).toBeNull()
  expect(respondedIpcRequest?.sessionId).toBe("ses_42")
})

test("daemon submit request honors repository-local security deny policy", async () => {
  await useTempHome()
  const repoDir = await createGitRepoFixture({
    owner: "trusted",
    repo: "widgets",
    branch: "feature/secure-daemon",
  })
  await writeLocalRootConfig(repoDir, {
    security: {
      pullRequests: {
        submit: "deny",
      },
    },
  })

  const createCalls: Array<Record<string, unknown>> = []
  const daemon = await startServer({
    sdk: {
      pr: {
        create: async (input) => {
          createCalls.push(input)
          return {
            number: 42,
            url: "https://github.com/trusted/widgets/pull/42",
          }
        },
      },
    },
    useExistingHome: true,
  })
  seedAuthorizedSession({
    sessionId: "ses_policy",
    token: "tok_session",
    owner: "trusted",
    repo: "widgets",
    allowedPrNumbers: [],
  })

  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  await expect(
    send(client, "pr.submit", {
      token: "tok_session",
      cwd: repoDir,
      title: "Ship daemon security",
      body: "Done.",
    }),
  ).rejects.toThrow(/disabled by security policy/i)
  expect(createCalls).toEqual([])
})

test("daemon reply request rejects PRs outside the session allowlist", async () => {
  await useTempHome()
  const repoDir = await createGitRepoFixture({
    owner: "evil",
    repo: "fork",
    branch: "pr-12",
  })

  const daemon = await startServer({ useExistingHome: true })
  seedAuthorizedSession({
    sessionId: "ses_7",
    token: "tok_session",
    owner: "trusted",
    repo: "widgets",
    allowedPrNumbers: [7],
  })

  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  await expect(
    send(client, "pr.reply", {
      token: "tok_session",
      cwd: repoDir,
      message: "Updated per review",
    }),
  ).rejects.toThrow(/not allowed/i)
})

test("daemon reply request records pull request checkout locations", async () => {
  await useTempHome()
  const repoDir = await createGitRepoFixture({
    owner: "evil",
    repo: "fork",
    branch: "pr-12",
  })

  const daemon = await startServer({ useExistingHome: true })
  seedAuthorizedSession({
    sessionId: "ses_12",
    token: "tok_session",
    owner: "trusted",
    repo: "widgets",
    allowedPrNumbers: [12],
  })

  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  await send(client, "pr.reply", {
    token: "tok_session",
    cwd: repoDir,
    message: "Updated per review",
  })

  expect(
    db.pullRequests.findMany().map(({ host, owner, repo, prNumber, cwd }: DaemonPullRequest) => ({
      host,
      owner,
      repo,
      prNumber,
      cwd,
    })),
  ).toEqual([
    {
      host: "github",
      owner: "trusted",
      repo: "widgets",
      prNumber: 12,
      cwd: repoDir,
    },
  ])
  const pullRequest = db.pullRequests.first({
    where: {
      host: "github",
      owner: "trusted",
      repo: "widgets",
      prNumber: 12,
    },
  })
  expect(db.inboxItems.first({ where: { entityId: pullRequest!.id } })).toMatchObject({
    entityId: pullRequest!.id,
    reason: "pull_request.updated",
    status: "unread",
    priority: "normal",
    scope: "Session",
    headline: "PR reply posted",
  })
})

test("daemon session reporting creates and updates session inbox rows", async () => {
  await useTempHome()
  const daemon = await startServer({ useExistingHome: true })
  seedAuthorizedSession({
    sessionId: "ses_inbox",
    token: "tok_session",
    owner: "trusted",
    repo: "widgets",
    allowedPrNumbers: [],
  })

  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const inboxEvents: Array<{
    status: string
  }> = []
  const unsubscribe = await subscribe(client, "inbox.streamItems", (item) => {
    if (item.entityId === "ses_inbox") {
      inboxEvents.push({
        status: item.status,
      })
    }
  })
  cleanup.push(async () => {
    unsubscribe()
  })

  await send(client, "session.reportTurnEnded", {
    id: "ses_inbox",
    scope: "Checkout flow",
    headline: "Decision ready for review",
  })

  const turnEndedItem = db.inboxItems.first({ where: { entityId: "ses_inbox" } })
  expect(turnEndedItem).toMatchObject({
    entityId: "ses_inbox",
    reason: "session.turn_ended",
    status: "unread",
    scope: "Checkout flow",
    headline: "Decision ready for review",
  })

  await send(client, "inbox.update", {
    entityId: "ses_inbox",
    status: "read",
  })
  expect(db.inboxItems.first({ where: { entityId: "ses_inbox" } })?.status).toBe("read")
  await send(client, "inbox.completeSession", { id: "ses_inbox" })
  expect(db.inboxItems.first({ where: { entityId: "ses_inbox" } })?.status).toBe("completed")
  await waitFor(async () => inboxEvents.length >= 3)
  expect(inboxEvents).toEqual([{ status: "unread" }, { status: "read" }, { status: "completed" }])
})

test("daemon workforce request rejects mismatched roots for token-backed sessions", async () => {
  await useTempHome()
  const sessionId = db.sessions.newId()
  const token = "workforce-token-mismatch"
  const rootDir = await mkdtemp(join(tmpdir(), "goddard-workforce-root-a-"))
  const otherRootDir = await mkdtemp(join(tmpdir(), "goddard-workforce-root-b-"))
  cleanup.push(() => removeTemporaryPath(rootDir))
  cleanup.push(() => removeTemporaryPath(otherRootDir))

  const daemon = await startServer({ useExistingHome: true })
  await seedWorkforceSession({
    sessionId,
    token,
    rootDir,
    requestId: "req-mismatch",
  })

  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  await expect(
    send(client, "workforce.request", {
      rootDir: otherRootDir,
      targetAgentId: "root",
      input: "Ship it.",
      token,
    }),
  ).rejects.toThrow(/does not match requested root/i)
})

test("daemon workforce respond rejects mismatched roots for token-backed sessions", async () => {
  await useTempHome()
  const sessionId = db.sessions.newId()
  const token = "workforce-token-respond"
  const rootDir = await mkdtemp(join(tmpdir(), "goddard-workforce-root-c-"))
  const otherRootDir = await mkdtemp(join(tmpdir(), "goddard-workforce-root-d-"))
  cleanup.push(() => removeTemporaryPath(rootDir))
  cleanup.push(() => removeTemporaryPath(otherRootDir))

  const daemon = await startServer({ useExistingHome: true })
  await seedWorkforceSession({
    sessionId,
    token,
    rootDir,
    requestId: "req-respond",
  })

  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  await expect(
    send(client, "workforce.respond", {
      rootDir: otherRootDir,
      output: "done",
      token,
    }),
  ).rejects.toThrow(/does not match requested root/i)
})

test("daemon workforce request rejects token-backed sessions without a workforce root", async () => {
  await useTempHome()
  const sessionId = db.sessions.newId()
  const token = "workforce-token-no-root"
  const rootDir = await mkdtemp(join(tmpdir(), "goddard-workforce-root-e-"))
  cleanup.push(() => removeTemporaryPath(rootDir))

  const daemon = await startServer({ useExistingHome: true })
  await seedWorkforceSession({
    sessionId,
    token,
    rootDir,
    requestId: "req-no-root",
    includeRootDir: false,
  })

  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  await expect(
    send(client, "workforce.request", {
      rootDir,
      targetAgentId: "root",
      input: "Ship it.",
      token,
    }),
  ).rejects.toThrow(/not attached to a workforce root/i)
})

type StartServerOptions = {
  useExistingHome?: boolean
  sdk?: {
    auth?: {
      device?: {
        start?: (input?: any) => Promise<any>
        complete?: (input: any) => Promise<any>
      }
      whoami?: () => Promise<any>
    }
    pr?: {
      create?: (input: any) => Promise<any>
      reply?: (input: any) => Promise<any>
    }
  }
}

async function startServer(options: StartServerOptions = {}): Promise<DaemonServer> {
  if (!options.useExistingHome) {
    await useTempHome()
  }

  const daemon = await startDaemonServer(
    createTestBackendClient({
      auth: {
        start:
          options.sdk?.auth?.device?.start ??
          (async (_input?: any) => ({
            deviceCode: "dev_1",
            userCode: "ABCD-1234",
            verificationUri: "https://github.com/login/device",
            expiresIn: 900,
            interval: 5,
          })),
        complete:
          options.sdk?.auth?.device?.complete ??
          (async (_input: any) => ({
            token: "tok_1",
            githubUsername: "alec",
            githubUserId: 42,
          })),
        current:
          options.sdk?.auth?.whoami ??
          (async () => ({
            token: "tok_1",
            githubUsername: "alec",
            githubUserId: 42,
          })),
      },
      pullRequests: {
        create:
          options.sdk?.pr?.create ??
          (async (_input: any) => ({
            number: 12,
            url: "https://github.com/trusted/widgets/pull/12",
          })),
        reply: options.sdk?.pr?.reply ?? (async (_input: any) => ({ success: true })),
      },
    }),
    { port: 0, store: db },
  )

  cleanup.push(async () => {
    await daemon.close()
  })

  return daemon
}

function createTestBackendClient(
  input: {
    auth?: {
      start?: (input?: any) => Promise<any>
      complete?: (input: any) => Promise<any>
      current?: () => Promise<any>
    }
    pullRequests?: {
      create?: (input: any) => Promise<any>
      reply?: (input: any) => Promise<any>
    }
  } = {},
): BackendClient {
  return {
    auth: {
      device: {
        start: (body?: any) => input.auth?.start?.(body),
        complete: (body: any) => input.auth?.complete?.(body),
      },
      session: {
        current: () => input.auth?.current?.(),
      },
    },
    pullRequests: {
      create: (body: any) => input.pullRequests?.create?.(body),
      managed: async () => ({ managed: true }),
      comments: {
        create: (body: any) => input.pullRequests?.reply?.(body),
      },
    },
    webhooks: {
      github: async () => ({ type: "noop" }),
    },
    remoteRepo: {
      stream: async () => new Response(),
    },
    stream: {
      subscribe: async () => {
        throw new Error("not used")
      },
    },
  } as unknown as BackendClient
}

async function useTempHome(): Promise<void> {
  sharedHomeDir ??= await mkdtemp(join(tmpdir(), "goddard-daemon-ipc-home-"))
  process.env.HOME = sharedHomeDir
  db = resetComposedDaemonStore()
}

async function writeLocalRootConfig(repoDir: string, config: Record<string, unknown>) {
  const configDir = join(repoDir, ".goddard")
  await mkdir(configDir, { recursive: true })
  await writeFile(
    join(configDir, "config.json"),
    `${JSON.stringify(
      {
        $schema:
          "https://raw.githubusercontent.com/goddard-ai/core/refs/heads/main/schema/json/goddard.json",
        ...config,
      },
      null,
      2,
    )}\n`,
    "utf8",
  )
}

async function writeGlobalRootConfig(config: Record<string, unknown>) {
  const configPath = getGlobalConfigPath()
  await mkdir(dirname(configPath), { recursive: true })
  await writeFile(
    configPath,
    `${JSON.stringify(
      {
        $schema:
          "https://raw.githubusercontent.com/goddard-ai/core/refs/heads/main/schema/json/goddard.json",
        ...config,
      },
      null,
      2,
    )}\n`,
    "utf8",
  )
}

function browserFetch(
  daemon: DaemonServer,
  path: string,
  input: {
    method: "GET" | "OPTIONS" | "POST"
    origin?: string
    privateNetwork?: boolean
    token?: string
    body?: unknown
  },
) {
  const url = new URL(path, daemon.daemonUrl)
  const headers: Record<string, string> = {
    Host: url.host,
    Connection: "close",
  }
  if (input.origin !== undefined) {
    headers.Origin = input.origin
  }
  if (input.privateNetwork) {
    headers["Access-Control-Request-Private-Network"] = "true"
  }
  if (input.token) {
    headers.Authorization = `Bearer ${input.token}`
  }
  const body = input.body === undefined ? undefined : JSON.stringify(input.body)
  if (input.body !== undefined) {
    headers["Content-Type"] = "application/json"
    headers["Content-Length"] = String(Buffer.byteLength(body ?? ""))
  }

  return new Promise<Response>((resolve, reject) => {
    const socket = connect(Number(url.port), url.hostname)
    const chunks: Buffer[] = []
    socket.on("error", reject)
    socket.on("connect", () => {
      socket.write(
        [
          `${input.method} ${url.pathname} HTTP/1.1`,
          ...Object.entries(headers).map(([name, value]) => `${name}: ${value}`),
          "",
          body ?? "",
        ].join("\r\n"),
      )
    })
    socket.on("data", (chunk) => {
      chunks.push(Buffer.from(chunk))
    })
    socket.on("end", () => {
      const raw = Buffer.concat(chunks).toString("utf8")
      const [head = "", framedBody = ""] = raw.split("\r\n\r\n")
      const [statusLine = "", ...headerLines] = head.split("\r\n")
      const status = Number(statusLine.split(" ")[1] ?? 0)
      const responseHeaders = new Headers()
      for (const line of headerLines) {
        const separator = line.indexOf(":")
        if (separator > 0) {
          responseHeaders.set(line.slice(0, separator), line.slice(separator + 1).trim())
        }
      }
      const responseBody =
        responseHeaders.get("transfer-encoding") === "chunked"
          ? decodeChunkedBody(framedBody)
          : framedBody
      resolve(
        new Response(responseBody, {
          status,
          headers: responseHeaders,
        }),
      )
    })
  })
}

function createOriginFetch(origin: string): typeof fetch {
  return (async (input: Parameters<typeof fetch>[0], init?: Parameters<typeof fetch>[1]) => {
    const headers = new Headers(init?.headers)
    headers.set("Origin", origin)
    return await fetch(input, {
      ...init,
      headers,
    })
  }) as unknown as typeof fetch
}

function decodeChunkedBody(framedBody: string) {
  let rest = framedBody
  let decoded = ""

  while (rest) {
    const separator = rest.indexOf("\r\n")
    if (separator < 0) {
      return decoded
    }

    const length = Number.parseInt(rest.slice(0, separator), 16)
    if (!Number.isFinite(length) || length <= 0) {
      return decoded
    }

    const start = separator + 2
    decoded += rest.slice(start, start + length)
    rest = rest.slice(start + length + 2)
  }

  return decoded
}

async function browserFetchJson<T>(
  daemon: DaemonServer,
  path: string,
  input: Parameters<typeof browserFetch>[2],
) {
  const response = await browserFetch(daemon, path, input)
  expect(response.ok).toBe(true)
  return (await response.json()) as T
}

async function seedWorkforceSession(input: {
  sessionId: DaemonSession["id"]
  token: string
  rootDir: string
  requestId: string
  includeRootDir?: boolean
}): Promise<void> {
  const sessionRecord = {
    acpSessionId: `acp-${input.sessionId}`,
    status: "active",
    stopReason: null,
    agent: "pi-acp",
    agentName: "pi",
    cwd: input.rootDir,
    title: "New session",
    titleState: "placeholder",
    lastSessionActivityAt: 1_776_000_000_000,
    mcpServers: [],
    connectionMode: "none",
    supportsLoadSession: false,
    activeDaemonSession: false,
    completedHidden: false,
    errorMessage: null,
    blockedReason: null,
    initiative: null,
    inboxScope: null,
    lastAgentMessage: null,
    repository: null,
    prNumber: null,
    token: input.token,
    permissions: {
      owner: "trusted",
      repo: "widgets",
      allowedPrNumbers: [],
    },
    metadata: null,
    configOptions: [],
    availableCommands: [],
    contextUsage: null,
  } satisfies Parameters<typeof db.sessions.put>[1]
  db.sessions.put(input.sessionId, sessionRecord)
  db.workforces.create({
    sessionId: input.sessionId,
    rootDir: input.includeRootDir === false ? undefined : input.rootDir,
    agentId: "root",
    requestId: input.requestId,
  })
}

function seedAuthorizedSession(input: {
  sessionId: DaemonSession["id"]
  token: string
  owner: string
  repo: string
  allowedPrNumbers: number[]
}) {
  db.sessions.put(input.sessionId, {
    acpSessionId: `acp-${input.sessionId}`,
    status: "active",
    stopReason: null,
    agent: "pi-acp",
    agentName: "pi",
    cwd: process.cwd(),
    title: "New session",
    titleState: "placeholder",
    lastSessionActivityAt: 1_776_000_000_000,
    mcpServers: [],
    connectionMode: "none",
    supportsLoadSession: false,
    activeDaemonSession: false,
    completedHidden: false,
    errorMessage: null,
    blockedReason: null,
    initiative: null,
    inboxScope: null,
    lastAgentMessage: null,
    repository: null,
    prNumber: null,
    token: input.token,
    permissions: {
      owner: input.owner,
      repo: input.repo,
      allowedPrNumbers: input.allowedPrNumbers,
    },
    metadata: null,
    configOptions: [],
    availableCommands: [],
    contextUsage: null,
  })
}

async function createGitRepoFixture(input: {
  owner: string
  repo: string
  branch: string
}): Promise<string> {
  const repoDir = await mkdtemp(join(tmpdir(), "goddard-daemon-ipc-repo-"))
  cleanup.push(() => removeTemporaryPath(repoDir))
  await writeFile(join(repoDir, "README.md"), "# fixture\n", "utf8")
  runGit(repoDir, ["init"])
  runGit(repoDir, ["config", "user.email", "bot@example.com"])
  runGit(repoDir, ["config", "user.name", "Bot"])
  runGit(repoDir, ["add", "README.md"])
  runGit(repoDir, ["commit", "-m", "init"])
  runGit(repoDir, ["checkout", "-b", input.branch])
  runGit(repoDir, [
    "remote",
    "add",
    "origin",
    `https://github.com/${input.owner}/${input.repo}.git`,
  ])
  return repoDir
}

function runGit(cwd: string, args: string[]) {
  const result = spawnSync("git", args, {
    cwd,
    encoding: "utf8",
  })

  expect(result.status).toBe(0)
}

async function captureLogs(
  action: () => Promise<void>,
): Promise<{ logs: Array<Record<string, unknown>> }> {
  const output: string[] = []
  const store = createLogStore({ databasePath: ":memory:" })
  const restoreLogging = configureLogging({
    mode: "json",
    writeLine: (line) => {
      output.push(line)
    },
    store,
  })

  try {
    await action()
    const debugLogs = store.query({ debugScope: "ipc.server", limit: 1_000 }).map((entry) => ({
      event: entry.message,
      level: entry.level,
      ...entry.properties,
    }))
    return {
      logs: [
        ...output
          .flatMap((chunk) => chunk.split("\n"))
          .filter((line) => line.trim().length > 0)
          .map((line) => JSON.parse(line) as Record<string, unknown>),
        ...debugLogs,
      ],
    }
  } finally {
    restoreLogging()
    store.close()
  }
}

async function waitFor(check: () => Promise<boolean> | boolean, timeoutMs = 2_000) {
  const start = Date.now()
  while (Date.now() - start < timeoutMs) {
    if (await check()) {
      return
    }
    await new Promise((resolve) => setTimeout(resolve, 25))
  }

  throw new Error("Timed out waiting for condition")
}
