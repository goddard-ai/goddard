import { expect, test } from "vitest"
import { GoddardSdk } from "../src/sdk.ts"

test("sdk auth methods forward plain-object payloads to the injected daemon client", async () => {
  const calls: Array<{ method: string; payload: unknown }> = []
  const sdk = new GoddardSdk({
    client: {
      async send(method: string, payload: unknown) {
        calls.push({ method, payload })

        if (method === "authDeviceStart") {
          return {
            deviceCode: "dev_1",
            userCode: "ABCD-1234",
            verificationUri: "https://github.com/login/device",
            expiresIn: 900,
            interval: 5,
          }
        }

        if (method === "authDeviceComplete") {
          return {
            token: "tok_1",
            githubUsername: "alec",
            githubUserId: 42,
          }
        }

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

  await expect(sdk.auth.startDeviceFlow({ githubUsername: "alec" })).resolves.toMatchObject({
    deviceCode: "dev_1",
  })
  await expect(
    sdk.auth.completeDeviceFlow({
      deviceCode: "dev_1",
      githubUsername: "alec",
    }),
  ).resolves.toMatchObject({ githubUserId: 42 })
  await expect(sdk.auth.whoami({})).resolves.toMatchObject({ githubUserId: 42 })
  await expect(sdk.auth.logout({})).resolves.toEqual({ success: true })
  expect(calls).toEqual([
    { method: "authDeviceStart", payload: { githubUsername: "alec" } },
    {
      method: "authDeviceComplete",
      payload: { deviceCode: "dev_1", githubUsername: "alec" },
    },
    { method: "authWhoami", payload: {} },
    { method: "authLogout", payload: {} },
  ])
})

test("sdk namespace getters cache their objects and return raw daemon responses", async () => {
  const sdk = new GoddardSdk({
    client: {
      async send(method: string, payload: unknown) {
        if (method === "workforceList") {
          expect(payload).toEqual({})
          return {
            workforces: [
              {
                state: "running",
                rootDir: "/repo",
                configPath: "/repo/.goddard/workforce/config.json",
                ledgerPath: "/repo/.goddard/workforce/ledger.jsonl",
                requestCount: 0,
                activeRequestCount: 0,
                suspendedRequestCount: 0,
                queuedRequestCount: 0,
                completedRequestCount: 0,
                cancelledRequestCount: 0,
                rootAgentId: "root",
                updatedAt: "2026-01-01T00:00:00.000Z",
              },
            ],
          }
        }

        throw new Error(`Unexpected daemon method: ${method}`)
      },
    } as never,
  })

  expect(sdk.workforce).toBe(sdk.workforce)
  await expect(sdk.workforce.list({})).resolves.toEqual({
    workforces: [
      expect.objectContaining({
        rootDir: "/repo",
        rootAgentId: "root",
      }),
    ],
  })
})
