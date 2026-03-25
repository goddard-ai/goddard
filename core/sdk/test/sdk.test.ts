import { expect, test, vi } from "vitest"
import { GoddardSdk } from "../src/sdk.ts"

test("sdk auth delegates device-flow lifecycle to daemon IPC", async () => {
  const send = vi.fn(async (method: string, payload: unknown) => {
    if (method === "authDeviceStart") {
      expect(payload).toEqual({ githubUsername: "alec" })
      return {
        deviceCode: "dev_1",
        userCode: "ABCD-1234",
        verificationUri: "https://github.com/login/device",
        expiresIn: 900,
        interval: 5,
      }
    }

    if (method === "authDeviceComplete") {
      expect(payload).toEqual({
        deviceCode: "dev_1",
        githubUsername: "alec",
      })
      return {
        token: "tok_1",
        githubUsername: "alec",
        githubUserId: 42,
      }
    }

    throw new Error(`Unexpected daemon method: ${method}`)
  })

  const sdk = new GoddardSdk({
    client: { send } as never,
  })

  const start = await sdk.auth.startDeviceFlow({ githubUsername: "alec" })
  expect(start.deviceCode).toBe("dev_1")

  const session = await sdk.auth.completeDeviceFlow({
    deviceCode: start.deviceCode,
    githubUsername: "alec",
  })
  expect(session.githubUserId).toBe(42)
})

test("sdk login polls through daemon auth methods until completion", async () => {
  let completionAttempts = 0
  const send = vi.fn(async (method: string) => {
    if (method === "authDeviceStart") {
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
  })

  const sdk = new GoddardSdk({
    client: { send } as never,
  })

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
  expect(session.githubUsername).toBe("alec")
  expect(completionAttempts).toBe(2)
})

test("sdk auth whoami and logout use daemon IPC", async () => {
  const send = vi.fn(async (method: string) => {
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
  })

  const sdk = new GoddardSdk({
    client: { send } as never,
  })

  await expect(sdk.auth.whoami()).resolves.toMatchObject({ githubUserId: 42 })
  await expect(sdk.auth.logout()).resolves.toBeUndefined()
  expect(send).toHaveBeenNthCalledWith(1, "authWhoami", {})
  expect(send).toHaveBeenNthCalledWith(2, "authLogout", {})
})
