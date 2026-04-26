import { expect, test } from "bun:test"

import {
  CloudSession,
  CloudSessionCoordinator,
  type CloudSessionHarnessSocket,
} from "../src/cloud-session.ts"

test("cloud session coordinator creates an ordered sync log", async () => {
  const coordinator = new CloudSessionCoordinator()

  const created = await coordinator.createSession({
    sessionId: "cls_test",
    metadata: { provider: "blaxel" },
  })
  const sync = await coordinator.sync(0)

  expect(created.session.id).toBe("cls_test")
  expect(created.session.metadata).toEqual({ provider: "blaxel" })
  expect(created.events).toHaveLength(1)
  expect(created.events[0].seq).toBe(1)
  expect(created.events[0].type).toBe("cloud-session.created")
  expect(sync.cursor).toBe(1)
  expect(sync.hasMore).toBe(false)
  expect(sync.events.map((event) => event.type)).toEqual(["cloud-session.created"])
})

test("cloud session coordinator deduplicates daemon commands", async () => {
  const coordinator = new CloudSessionCoordinator()
  await coordinator.createSession({ sessionId: "cls_commands" })

  const command = {
    commandId: "cmd_1",
    type: "prompt" as const,
    payload: { prompt: "Implement this" },
  }
  const first = await coordinator.enqueueCommand(command)
  const duplicate = await coordinator.enqueueCommand(command)
  const sync = await coordinator.sync(0)

  expect(first.duplicate).toBe(false)
  expect(duplicate.duplicate).toBe(true)
  expect(duplicate.event?.seq).toBe(first.event?.seq)
  expect(
    sync.events.filter((event) => event.type === "cloud-session.command.accepted"),
  ).toHaveLength(1)
})

test("cloud session coordinator fences harness channels and records harness events", async () => {
  const coordinator = new CloudSessionCoordinator()
  await coordinator.createSession({ sessionId: "cls_harness" })

  const firstHarness = new CapturingHarnessSocket()
  const firstAttach = await coordinator.attachHarness(firstHarness)
  const secondHarness = new CapturingHarnessSocket()
  const secondAttach = await coordinator.attachHarness(secondHarness)

  await coordinator.enqueueCommand({
    commandId: "cmd_harness_1",
    type: "prompt",
    payload: { prompt: "Continue" },
  })
  await coordinator.ingestHarnessMessage({
    type: "event",
    eventType: "session/update",
    payload: { update: "working" },
  })
  await coordinator.ingestHarnessMessage({
    type: "status",
    status: "idle",
    sandboxStatus: "ready",
  })

  const sync = await coordinator.sync(0)
  const deliveredCommand = JSON.parse(secondHarness.messages[1]) as {
    type: string
    harnessEpoch: number
    command: { commandId: string }
  }

  expect(firstAttach.session.harnessEpoch).toBe(1)
  expect(secondAttach.session.harnessEpoch).toBe(2)
  expect(firstHarness.closed).toEqual({ code: 1012, reason: "Harness superseded" })
  expect(deliveredCommand.type).toBe("command")
  expect(deliveredCommand.harnessEpoch).toBe(2)
  expect(deliveredCommand.command.commandId).toBe("cmd_harness_1")
  expect(sync.session.status).toBe("idle")
  expect(sync.events.map((event) => event.type)).toContain("session/update")
  expect(sync.events.map((event) => event.type)).toContain("cloud-session.status")
})

test("cloud session durable object exposes create, sync, and command endpoints", async () => {
  const cloudSession = new CloudSession()

  const createResponse = await cloudSession.fetch(
    new Request("https://cloud-session.internal/create", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ sessionId: "cls_do" }),
    }),
  )
  const commandResponse = await cloudSession.fetch(
    new Request("https://cloud-session.internal/commands", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        commandId: "cmd_do_1",
        type: "initialize",
        payload: { protocol: "acp" },
      }),
    }),
  )
  const duplicateResponse = await cloudSession.fetch(
    new Request("https://cloud-session.internal/commands", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        commandId: "cmd_do_1",
        type: "initialize",
        payload: { protocol: "acp" },
      }),
    }),
  )
  const syncResponse = await cloudSession.fetch(
    new Request("https://cloud-session.internal/sync?after=0"),
  )

  expect(createResponse.status).toBe(200)
  expect(commandResponse.status).toBe(200)
  expect(duplicateResponse.status).toBe(200)
  const duplicate = (await duplicateResponse.json()) as { duplicate: boolean }
  const sync = (await syncResponse.json()) as {
    cursor: number
    events: Array<{ type: string }>
  }

  expect(duplicate.duplicate).toBe(true)
  expect(sync.cursor).toBe(2)
  expect(sync.events.map((event) => event.type)).toEqual([
    "cloud-session.created",
    "cloud-session.command.accepted",
  ])
})

class CapturingHarnessSocket implements CloudSessionHarnessSocket {
  messages: string[] = []
  closed: { code?: number; reason?: string } | undefined

  send(payload: string) {
    this.messages.push(payload)
  }

  close(code?: number, reason?: string) {
    this.closed = { code, reason }
  }
}
