import { afterEach, expect, test, vi } from "vitest"

import { createBrowserDaemonClient } from "./daemon-client.ts"

afterEach(() => {
  vi.unstubAllGlobals()
  window.__goddardDesktop = undefined as any
})

test("browser daemon client uses hosted browser pairing inputs when no desktop bridge exists", async () => {
  window.localStorage.setItem("goddard.daemonUrl", "http://127.0.0.1:49827/")
  window.localStorage.setItem("goddard.daemonBrowserToken", "hosted-token")
  const authorizations: Array<string | null> = []
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      authorizations.push(new Request(input, init).headers.get("authorization"))
      return Response.json({ ok: true })
    }),
  )

  const client = createBrowserDaemonClient()

  await expect(client.daemon.health()).resolves.toEqual({ ok: true })
  expect(authorizations).toEqual(["Bearer hosted-token"])
})

test("desktop browser daemon client refreshes a rejected webview token once", async () => {
  const tokens = ["expired-token", "fresh-token"]
  window.__goddardDesktop = {
    createDaemonWebviewAccessToken: vi.fn(async () => ({
      daemonUrl: "http://127.0.0.1:49827/",
      token: tokens.shift() ?? "unexpected-token",
      origin: window.location.origin,
      expiresAt: new Date(Date.now() + 60_000).toISOString(),
    })),
  } as any
  const authorizations: Array<string | null> = []
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const authorization = new Request(input, init).headers.get("authorization")
      authorizations.push(authorization)
      if (authorization === "Bearer expired-token") {
        return Response.json({ error: "Forbidden" }, { status: 403 })
      }

      return Response.json({ ok: true })
    }),
  )

  const client = createBrowserDaemonClient()

  await expect(client.daemon.health()).resolves.toEqual({ ok: true })
  expect(window.__goddardDesktop.createDaemonWebviewAccessToken).toHaveBeenCalledTimes(2)
  expect(window.__goddardDesktop.createDaemonWebviewAccessToken).toHaveBeenCalledWith(
    window.location.origin,
  )
  expect(authorizations).toEqual(["Bearer expired-token", "Bearer fresh-token"])
})
