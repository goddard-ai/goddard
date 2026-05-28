import { expect, test } from "bun:test"

import { GoddardSdk } from "../src/index.ts"

test("session.prompt reconnects before forwarding a structured ACP prompt message", async () => {
  const calls: Array<{ name: string; payload: unknown }> = []
  const sdk = new GoddardSdk({
    client: {
      send: async (name: string, payload: unknown) => {
        calls.push({ name, payload })
        if (name === "session.connect") {
          return {
            session: {
              acpSessionId: "acp-session-reconnected",
            },
          }
        }

        return { accepted: true }
      },
      session: {
        connect: async (input: unknown) => {
          calls.push({ name: "session.connect", payload: input })
          return {
            session: {
              acpSessionId: "acp-session-reconnected",
            },
          }
        },
        send: async (input: unknown) => {
          calls.push({ name: "session.send", payload: input })
          return { accepted: true }
        },
      },
      subscribe: async () => {
        return () => {}
      },
    } as never,
  })

  await expect(
    sdk.session.prompt({
      id: "ses_daemon-session-1",
      acpId: "acp-session-1",
      prompt: "Review the current diff.",
    }),
  ).resolves.toEqual({
    accepted: true,
  })

  expect(calls).toHaveLength(2)
  expect(calls[0]).toEqual({
    name: "session.connect",
    payload: {
      id: "ses_daemon-session-1",
    },
  })
  expect(calls[1]?.name).toBe("session.send")
  expect(calls[1]?.payload).toMatchObject({
    id: "ses_daemon-session-1",
    message: {
      jsonrpc: "2.0",
      method: "session/prompt",
      params: {
        sessionId: "acp-session-reconnected",
        prompt: [{ type: "text", text: "Review the current diff." }],
      },
    },
  })
})

test("session.respondPermission forwards a structured ACP permission response through session.send", async () => {
  const calls: Array<{ name: string; payload: unknown }> = []
  const sdk = new GoddardSdk({
    client: {
      send: async (name: string, payload: unknown) => {
        calls.push({ name, payload })
        return { accepted: true }
      },
      session: {
        send: async (input: unknown) => {
          calls.push({ name: "session.send", payload: input })
          return { accepted: true }
        },
      },
      subscribe: async () => {
        return () => {}
      },
    } as never,
  })

  await expect(
    sdk.session.respondPermission({
      id: "ses_daemon-session-1",
      requestId: "permission-1",
      outcome: {
        outcome: "selected",
        optionId: "allow-once",
      },
    }),
  ).resolves.toEqual({
    accepted: true,
  })

  expect(calls).toHaveLength(1)
  expect(calls[0]?.name).toBe("session.send")
  expect(calls[0]?.payload).toEqual({
    id: "ses_daemon-session-1",
    message: {
      jsonrpc: "2.0",
      id: "permission-1",
      result: {
        outcome: {
          outcome: "selected",
          optionId: "allow-once",
        },
      },
    },
  })
})
