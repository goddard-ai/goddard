import type { SessionId } from "@goddard-ai/sdk"
import { describe, expect, test, vi } from "vitest"

import { loadSessionChatComposerSuggestions } from "./composer-suggestion-loader.ts"

type LoaderSdk = Parameters<typeof loadSessionChatComposerSuggestions>[0]["sdk"]

function createSdk(): LoaderSdk {
  return {
    fileSearch: {
      composerEntries: vi.fn(async () => ({
        entries: [
          {
            type: "file" as const,
            path: "/project/src/index.ts",
            uri: "file:///project/src/index.ts",
            label: "index.ts",
            detail: "./src/index.ts",
          },
        ],
      })),
    },
    session: {
      composerSuggestions: vi.fn(async () => ({
        suggestions: [
          {
            type: "skill" as const,
            path: "/project/.agents/skills/review",
            uri: "file:///project/.agents/skills/review",
            label: "review",
            detail: ".agents/skills/review",
            source: "local" as const,
          },
        ],
      })),
    },
  }
}

describe("loadSessionChatComposerSuggestions", () => {
  test("routes @ suggestions to file search", async () => {
    const sdk = createSdk()

    const suggestions = await loadSessionChatComposerSuggestions({
      cwd: "/project",
      query: "index",
      sdk,
      sessionId: "ses_1" as SessionId,
      trigger: "at",
    })

    expect(sdk.fileSearch.composerEntries).toHaveBeenCalledWith({
      cwd: "/project",
      query: "index",
    })
    expect(sdk.session.composerSuggestions).not.toHaveBeenCalled()
    expect(suggestions).toEqual([
      {
        type: "file",
        path: "/project/src/index.ts",
        uri: "file:///project/src/index.ts",
        label: "index.ts",
        detail: "./src/index.ts",
      },
    ])
  })

  test("keeps $ and / suggestions on the session composer API", async () => {
    const sdk = createSdk()

    await loadSessionChatComposerSuggestions({
      cwd: "/project",
      query: "review",
      sdk,
      sessionId: "ses_1" as SessionId,
      trigger: "dollar",
    })
    await loadSessionChatComposerSuggestions({
      cwd: "/project",
      query: "run",
      sdk,
      sessionId: "ses_1" as SessionId,
      trigger: "slash",
    })

    expect(sdk.fileSearch.composerEntries).not.toHaveBeenCalled()
    expect(sdk.session.composerSuggestions).toHaveBeenCalledTimes(2)
    expect(sdk.session.composerSuggestions).toHaveBeenNthCalledWith(1, {
      id: "ses_1",
      trigger: "dollar",
      query: "review",
    })
    expect(sdk.session.composerSuggestions).toHaveBeenNthCalledWith(2, {
      id: "ses_1",
      trigger: "slash",
      query: "run",
    })
  })
})
