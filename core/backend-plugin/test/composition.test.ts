import { describe, expect, test } from "bun:test"

import {
  composeBackendEvents,
  defineBackendEvents,
  type BackendEventEnvelope,
} from "../src/index.ts"

describe("backend event composition", () => {
  test("composes backend event definition fragments", async () => {
    type PullRequestFeedbackEvent = BackendEventEnvelope<
      "pull_request.feedback.received",
      { owner: string; repo: string; prNumber: number },
      { provider: "github"; deliveryId: string }
    >

    const github = defineBackendEvents({
      "pull_request.feedback.received": {
        normalizeWebhook: (webhook: {
          deliveryId: string
          owner: string
          repo: string
          prNumber: number
        }): PullRequestFeedbackEvent => ({
          name: "pull_request.feedback.received",
          payload: {
            owner: webhook.owner,
            repo: webhook.repo,
            prNumber: webhook.prNumber,
          },
          provenance: {
            provider: "github",
            deliveryId: webhook.deliveryId,
          },
        }),
        authorize: ({ principal, event }) =>
          principal === `${event.payload.owner}/${event.payload.repo}`,
        matchesFilter: ({ event, filter }) => filter === event.payload.owner,
      },
    })
    const remoteRepo = defineBackendEvents({
      "remote_repo.connected": {
        authorize: () => true,
      },
    })

    const composition = composeBackendEvents([github, remoteRepo])
    const normalized = await composition["pull_request.feedback.received"].normalizeWebhook?.({
      deliveryId: "delivery-1",
      owner: "acme",
      repo: "widgets",
      prNumber: 12,
    })

    expect(Object.keys(composition).sort()).toEqual([
      "pull_request.feedback.received",
      "remote_repo.connected",
    ])
    expect(normalized).toEqual({
      name: "pull_request.feedback.received",
      payload: {
        owner: "acme",
        repo: "widgets",
        prNumber: 12,
      },
      provenance: {
        provider: "github",
        deliveryId: "delivery-1",
      },
    })
    expect(
      await composition["pull_request.feedback.received"].authorize({
        principal: "acme/widgets",
        event: normalized as PullRequestFeedbackEvent,
      }),
    ).toBe(true)
    expect(
      composition["pull_request.feedback.received"].matchesFilter?.({
        event: normalized as PullRequestFeedbackEvent,
        filter: "acme",
      }),
    ).toBe(true)
  })

  test("rejects duplicate backend event names", () => {
    const first = defineBackendEvents({
      "pull_request.feedback.received": {
        authorize: () => true,
      },
    })
    const second = defineBackendEvents({
      "pull_request.feedback.received": {
        authorize: () => true,
      },
    })

    expect(() => composeBackendEvents([first, second])).toThrow(
      "Duplicate backend event: pull_request.feedback.received",
    )
  })
})
