import { describe, expect, mock, test } from "bun:test"

import { loadSessionLaunchComposerSuggestions } from "./launch-suggestion-loader.ts"

type LoaderSdk = Parameters<typeof loadSessionLaunchComposerSuggestions>[0]["sdk"]

function createSdk(): LoaderSdk {
  return {
    fileSearch: {
      composerEntries: mock(async () => ({
        entries: [
          {
            type: "folder" as const,
            path: "/project/src",
            uri: "file:///project/src",
            label: "src",
            detail: "./src",
          },
        ],
      })),
    },
    session: {
      draftSuggestions: mock(async () => ({
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

describe("loadSessionLaunchComposerSuggestions", () => {
  test("routes @ suggestions to file search", async () => {
    const sdk = createSdk()

    const suggestions = await loadSessionLaunchComposerSuggestions({
      cwd: "/project",
      query: "src",
      sdk,
      slashCommands: [],
      trigger: "at",
    })

    expect(sdk.fileSearch.composerEntries).toHaveBeenCalledWith({
      cwd: "/project",
      query: "src",
    })
    expect(sdk.session.draftSuggestions).not.toHaveBeenCalled()
    expect(suggestions).toEqual([
      {
        type: "folder",
        path: "/project/src",
        uri: "file:///project/src",
        label: "src",
        detail: "./src",
      },
    ])
  })

  test("keeps $ suggestions on draft suggestions", async () => {
    const sdk = createSdk()

    await loadSessionLaunchComposerSuggestions({
      cwd: "/project",
      query: "review",
      sdk,
      slashCommands: [],
      trigger: "dollar",
    })

    expect(sdk.fileSearch.composerEntries).not.toHaveBeenCalled()
    expect(sdk.session.draftSuggestions).toHaveBeenCalledWith({
      cwd: "/project",
      trigger: "dollar",
      query: "review",
    })
  })

  test("keeps / suggestions on launch preview filtering", async () => {
    const sdk = createSdk()

    const suggestions = await loadSessionLaunchComposerSuggestions({
      cwd: "/project",
      query: "",
      sdk,
      slashCommands: [
        {
          type: "slash_command",
          name: "review",
          description: "Review the current diff",
          inputHint: null,
        },
      ],
      trigger: "slash",
    })

    expect(sdk.fileSearch.composerEntries).not.toHaveBeenCalled()
    expect(sdk.session.draftSuggestions).not.toHaveBeenCalled()
    expect(suggestions).toEqual([
      {
        type: "slash_command",
        name: "review",
        description: "Review the current diff",
        inputHint: null,
      },
    ])
  })
})
