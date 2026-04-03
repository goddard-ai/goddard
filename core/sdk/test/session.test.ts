import { expect, test } from "vitest"
import { GoddardSdk } from "../src/index.ts"

test("session.prompt forwards a structured ACP prompt message through sessionSend", async () => {
  const calls: Array<{ name: string; payload: unknown }> = []
  const sdk = new GoddardSdk({
    client: {
      send: async (name: string, payload: unknown) => {
        calls.push({ name, payload })
        return { accepted: true }
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

  expect(calls).toHaveLength(1)
  expect(calls[0]?.name).toBe("sessionSend")
  expect(calls[0]?.payload).toMatchObject({
    id: "ses_daemon-session-1",
    message: {
      jsonrpc: "2.0",
      method: "session/prompt",
      params: {
        sessionId: "acp-session-1",
        prompt: [{ type: "text", text: "Review the current diff." }],
      },
    },
  })
})
