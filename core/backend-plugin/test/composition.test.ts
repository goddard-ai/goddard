import { describe, expect, test } from "bun:test"
import { z } from "zod"

import {
  composeBackendEvents,
  composeBackendEventSources,
  defineBackendEvents,
  defineBackendEventSources,
  type BackendEventEnvelope,
} from "../src/index.ts"

describe("backend event composition", () => {
  test("composes provider-agnostic event definitions", () => {
    const remoteRepo = defineBackendEvents({
      "remote_repo.event.received": {
        payload: z.object({
          owner: z.string(),
          repo: z.string(),
          prNumber: z.number(),
        }),
        provenance: z.object({
          provider: z.literal("github"),
          deliveryId: z.string(),
        }),
      },
    })
    const backendHealth = defineBackendEvents({
      "backend.stream.degraded": {
        payload: z.object({
          reason: z.literal("unauthenticated"),
          errorMessage: z.string(),
        }),
      },
    })

    const composition = composeBackendEvents([remoteRepo, backendHealth])

    expect(Object.keys(composition).sort()).toEqual([
      "backend.stream.degraded",
      "remote_repo.event.received",
    ])
    expect(
      composition["remote_repo.event.received"].payload.safeParse({
        owner: "acme",
        repo: "widgets",
        prNumber: 12,
      }).success,
    ).toBe(true)
  })

  test("composes source-owned authorization separately from event definitions", async () => {
    type RemoteRepoEvent = BackendEventEnvelope<
      "remote_repo.event.received",
      { owner: string; repo: string; prNumber: number },
      { provider: "github"; deliveryId: string }
    >
    const events = composeBackendEvents([
      defineBackendEvents({
        "remote_repo.event.received": {
          payload: z.object({
            owner: z.string(),
            repo: z.string(),
            prNumber: z.number(),
          }),
        },
      }),
    ])
    const github = defineBackendEventSources({
      github: {
        produces: ["remote_repo.event.received"],
        authorize: ({ principal, event }: { principal: string; event: RemoteRepoEvent }) =>
          principal === `${event.payload.owner}/${event.payload.repo}`,
      },
    })

    const sources = composeBackendEventSources([github], events)
    const event: RemoteRepoEvent = {
      name: "remote_repo.event.received",
      payload: {
        owner: "acme",
        repo: "widgets",
        prNumber: 12,
      },
      provenance: {
        provider: "github",
        deliveryId: "delivery-1",
      },
    }

    expect(Object.keys(sources)).toEqual(["github"])
    expect(await sources.github.authorize({ principal: "acme/widgets", event })).toBe(true)
    expect(await sources.github.authorize({ principal: "acme/other", event })).toBe(false)
  })

  test("rejects duplicate backend event names", () => {
    const first = defineBackendEvents({
      "remote_repo.event.received": {
        payload: z.object({}),
      },
    })
    const second = defineBackendEvents({
      "remote_repo.event.received": {
        payload: z.object({}),
      },
    })

    expect(() => composeBackendEvents([first, second])).toThrow(
      "Duplicate backend event: remote_repo.event.received",
    )
  })

  test("rejects duplicate backend event source names", () => {
    const events = composeBackendEvents([
      defineBackendEvents({
        "remote_repo.event.received": {
          payload: z.object({}),
        },
      }),
    ])
    const first = defineBackendEventSources({
      github: {
        produces: ["remote_repo.event.received"],
        authorize: () => true,
      },
    })
    const second = defineBackendEventSources({
      github: {
        produces: ["remote_repo.event.received"],
        authorize: () => true,
      },
    })

    expect(() => composeBackendEventSources([first, second], events)).toThrow(
      "Duplicate backend event source: github",
    )
  })

  test("rejects sources that claim unknown event names", () => {
    const events = composeBackendEvents([
      defineBackendEvents({
        "remote_repo.event.received": {
          payload: z.object({}),
        },
      }),
    ])
    const github = defineBackendEventSources({
      github: {
        produces: ["pull_request.feedback.received"],
        authorize: () => true,
      },
    })

    expect(() => composeBackendEventSources([github], events)).toThrow(
      "Backend event source github produces unknown event: pull_request.feedback.received",
    )
  })
})
