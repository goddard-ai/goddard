import { expect, test } from "vitest"
import { GoddardSdk } from "../src/sdk.ts"

test("sdk login polls until the daemon completes device authentication", async () => {
  let completionAttempts = 0
  const client = {
    async send(method: string, payload: unknown) {
      if (method === "authDeviceStart") {
        expect(payload).toEqual({ githubUsername: "alec" })
        return {
          deviceCode: "dev_1",
          userCode: "ABCD-1234",
          verificationUri: "https://github.com/login/device",
          expiresIn: 900,
          interval: 0,
        }
      }

      if (method === "authDeviceComplete") {
        completionAttempts += 1
        if (completionAttempts === 1) {
          throw new Error("authorization_pending")
        }

        return {
          token: "tok_1",
          githubUsername: "alec",
          githubUserId: 42,
        }
      }

      throw new Error(`Unexpected daemon method: ${method}`)
    },
  }

  const sdk = new GoddardSdk({ client: client as never })
  let prompt: { verificationUri: string; userCode: string } | null = null

  const session = await sdk.auth.login({
    githubUsername: "alec",
    onPrompt(verificationUri, userCode) {
      prompt = { verificationUri, userCode }
    },
  })

  expect(prompt).toEqual({
    verificationUri: "https://github.com/login/device",
    userCode: "ABCD-1234",
  })
  expect(session.githubUserId).toBe(42)
  expect(completionAttempts).toBe(2)
})

test("sdk auth forwards whoami and logout to the injected daemon client", async () => {
  const calls: Array<{ method: string; payload: unknown }> = []
  const sdk = new GoddardSdk({
    client: {
      async send(method: string, payload: unknown) {
        calls.push({ method, payload })

        if (method === "authWhoami") {
          return {
            token: "tok_1",
            githubUsername: "alec",
            githubUserId: 42,
          }
        }

        if (method === "authLogout") {
          return { success: true }
        }

        throw new Error(`Unexpected daemon method: ${method}`)
      },
    } as never,
  })

  await expect(sdk.auth.whoami()).resolves.toMatchObject({ githubUserId: 42 })
  await expect(sdk.auth.logout()).resolves.toBeUndefined()
  expect(calls).toEqual([
    { method: "authWhoami", payload: {} },
    { method: "authLogout", payload: {} },
  ])
})
