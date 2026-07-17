import { describe, expect, test } from "bun:test"
import { z } from "zod"

import {
  composeBackendEvents,
  composeBackendEventSources,
  composeBackendPlugins,
  composeBackendProviders,
  defineBackendEvents,
  defineBackendEventSources,
  defineBackendPlugin,
  defineBackendProviders,
  defineBackendRoutes,
  getBackendProviderCapability,
  http,
  type BackendEventEnvelope,
} from "../src/index.ts"

describe("backend event composition", () => {
  test("composes provider-agnostic event definitions", () => {
    const remoteRepo = defineBackendEvents({
      "example.event.received": {
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
      "example.event.received",
    ])
    expect(
      composition["example.event.received"].payload.safeParse({
        owner: "acme",
        repo: "widgets",
        prNumber: 12,
      }).success,
    ).toBe(true)
  })

  test("composes source-owned authorization separately from event definitions", async () => {
    type RemoteRepoEvent = BackendEventEnvelope<
      "example.event.received",
      { owner: string; repo: string; prNumber: number },
      { provider: "github"; deliveryId: string }
    >
    const events = composeBackendEvents([
      defineBackendEvents({
        "example.event.received": {
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
        produces: ["example.event.received"],
        authorize: ({ principal, event }: { principal: string; event: RemoteRepoEvent }) =>
          principal === `${event.payload.owner}/${event.payload.repo}`,
      },
    })

    const sources = composeBackendEventSources([github], events)
    const event: RemoteRepoEvent = {
      name: "example.event.received",
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
    expect(
      await sources.github.authorize({
        principal: "acme/widgets",
        event,
        providers: {},
      }),
    ).toBe(true)
    expect(
      await sources.github.authorize({
        principal: "acme/other",
        event,
        providers: {},
      }),
    ).toBe(false)
  })

  test("rejects duplicate backend event names", () => {
    const first = defineBackendEvents({
      "example.event.received": {
        payload: z.object({}),
      },
    })
    const second = defineBackendEvents({
      "example.event.received": {
        payload: z.object({}),
      },
    })

    expect(() => composeBackendEvents([first, second])).toThrow(
      "Duplicate backend event: example.event.received",
    )
  })

  test("rejects duplicate backend event source names", () => {
    const events = composeBackendEvents([
      defineBackendEvents({
        "example.event.received": {
          payload: z.object({}),
        },
      }),
    ])
    const first = defineBackendEventSources({
      github: {
        produces: ["example.event.received"],
        authorize: () => true,
      },
    })
    const second = defineBackendEventSources({
      github: {
        produces: ["example.event.received"],
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
        "example.event.received": {
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

describe("backend provider capability composition", () => {
  test("composes provider-keyed capability definitions", async () => {
    const providers = composeBackendProviders([
      defineBackendProviders({
        github: {
          authorizeRemoteRepositoryAccess: ({ repository }) => repository.repo === "widgets",
        },
      }),
      defineBackendProviders({
        gitlab: {
          authorizeRemoteRepositoryAccess: ({ repository }) => repository.repo === "platform",
        },
      }),
    ])

    expect(Object.keys(providers).sort()).toEqual(["github", "gitlab"])
    await expect(
      Promise.resolve(
        providers.github.authorizeRemoteRepositoryAccess?.({
          principal: { id: "user_1", providerIdentities: [] },
          repository: { provider: "github", owner: "acme", repo: "widgets" },
        }),
      ),
    ).resolves.toBe(true)
  })

  test("rejects duplicate backend providers", () => {
    const first = defineBackendProviders({
      github: {
        authorizeRemoteRepositoryAccess: () => true,
      },
    })
    const second = defineBackendProviders({
      github: {
        authorizeRemoteRepositoryAccess: () => true,
      },
    })

    expect(() => composeBackendProviders([first, second])).toThrow(
      "Duplicate backend provider: github",
    )
  })

  test("looks up provider capabilities and reports missing providers", () => {
    const providers = composeBackendProviders([
      defineBackendProviders({
        github: {
          parseRepositoryUrl: () => ({ provider: "github", owner: "acme", repo: "widgets" }),
        },
      }),
    ])

    expect(getBackendProviderCapability(providers, "github")).toBe(providers.github)
    expect(() => getBackendProviderCapability(providers, "gitlab" as never)).toThrow(
      "Unknown backend provider: gitlab",
    )
  })
})

describe("backend plugin composition", () => {
  test("composes route, event, and source fragments from backend plugins", () => {
    const remoteRepo = defineBackendPlugin({
      name: "remote-repo",
      routes: defineBackendRoutes({
        remoteRepo: http.resource("remote-repo", {
          stream: http.get("stream", {}),
        }),
      }),
      events: defineBackendEvents({
        "remote_repo.pull_request.comment.created": {
          payload: z.object({
            owner: z.string(),
            repo: z.string(),
            prNumber: z.number(),
            body: z.string(),
          }),
        },
      }),
      eventSources: defineBackendEventSources({
        "remote-repo": {
          produces: ["remote_repo.pull_request.comment.created"],
          authorize: () => true,
        },
      }),
    })
    const github = defineBackendPlugin({
      name: "github",
      routes: defineBackendRoutes({
        webhooks: http.resource("webhooks", {
          github: http.post("github", {
            body: http.rawBody(),
          }),
        }),
      }),
      providers: defineBackendProviders({
        github: {
          parseRepositoryUrl: () => ({ provider: "github", owner: "acme", repo: "widgets" }),
        },
      }),
    })

    const composition = composeBackendPlugins([remoteRepo, github])

    expect(composition.routes.remoteRepo.path.source).toBe("/remote-repo")
    expect(composition.routes.webhooks.children.github.path?.source).toBe("/github")
    expect(Object.keys(composition.events)).toEqual(["remote_repo.pull_request.comment.created"])
    expect(Object.keys(composition.eventSources)).toEqual(["remote-repo"])
    expect(Object.keys(composition.providers)).toEqual(["github"])
  })

  test("rejects duplicate backend plugin names", () => {
    const first = defineBackendPlugin({ name: "github" })
    const second = defineBackendPlugin({ name: "github" })

    expect(() => composeBackendPlugins([first, second])).toThrow("Duplicate backend plugin: github")
  })

  test("rejects duplicate backend plugin event definitions", () => {
    const first = defineBackendPlugin({
      name: "remote-repo-a",
      events: defineBackendEvents({
        "remote_repo.pull_request.comment.created": {
          payload: z.object({}),
        },
      }),
    })
    const second = defineBackendPlugin({
      name: "remote-repo-b",
      events: defineBackendEvents({
        "remote_repo.pull_request.comment.created": {
          payload: z.object({}),
        },
      }),
    })

    expect(() => composeBackendPlugins([first, second])).toThrow(
      "Duplicate backend event: remote_repo.pull_request.comment.created",
    )
  })

  test("rejects duplicate backend plugin event sources", () => {
    const events = defineBackendEvents({
      "remote_repo.pull_request.comment.created": {
        payload: z.object({}),
      },
    })
    const first = defineBackendPlugin({
      name: "remote-repo-a",
      events,
      eventSources: defineBackendEventSources({
        "remote-repo": {
          produces: ["remote_repo.pull_request.comment.created"],
          authorize: () => true,
        },
      }),
    })
    const second = defineBackendPlugin({
      name: "remote-repo-b",
      eventSources: defineBackendEventSources({
        "remote-repo": {
          produces: ["remote_repo.pull_request.comment.created"],
          authorize: () => true,
        },
      }),
    })

    expect(() => composeBackendPlugins([first, second])).toThrow(
      "Duplicate backend event source: remote-repo",
    )
  })

  test("rejects backend plugin event sources that claim unknown events", () => {
    const github = defineBackendPlugin({
      name: "github",
      eventSources: defineBackendEventSources({
        github: {
          produces: ["remote_repo.pull_request.comment.created"],
          authorize: () => true,
        },
      }),
    })

    expect(() => composeBackendPlugins([github])).toThrow(
      "Backend event source github produces unknown event: remote_repo.pull_request.comment.created",
    )
  })

  test("rejects duplicate backend plugin provider capabilities", () => {
    const first = defineBackendPlugin({
      name: "github-a",
      providers: defineBackendProviders({
        github: {
          authorizeRemoteRepositoryAccess: () => true,
        },
      }),
    })
    const second = defineBackendPlugin({
      name: "github-b",
      providers: defineBackendProviders({
        github: {
          authorizeRemoteRepositoryAccess: () => true,
        },
      }),
    })

    expect(() => composeBackendPlugins([first, second])).toThrow(
      "Duplicate backend provider: github",
    )
  })
})
