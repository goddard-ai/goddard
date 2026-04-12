import type { SessionPromptRequest } from "@goddard-ai/sdk"
import { expect, test } from "bun:test"
import {
  $createParagraphNode,
  $createTextNode,
  $getRoot,
  createEditor,
  type EditorState,
} from "lexical"

import { $createComposerChipNode, ComposerChipNode } from "./composer-chip-node.tsx"
import {
  hasPromptContent,
  promptBlocksToTranscriptContent,
  serializeComposerEditorState,
} from "./composer-content.ts"

type ComposerPromptBlock = Exclude<SessionPromptRequest["prompt"], string>[number]

function buildEditorState(build: () => void): EditorState {
  const editor = createEditor({
    nodes: [ComposerChipNode],
    onError(error) {
      throw error
    },
  })

  editor.update(build, { discrete: true })
  return editor.getEditorState()
}

test("serializeComposerEditorState preserves text order, slash chips, and resource links", () => {
  const editorState = buildEditorState(() => {
    const paragraph = $createParagraphNode()
    paragraph.append(
      $createTextNode("Review "),
      $createComposerChipNode({
        kind: "slash_command",
        label: "plan",
        description: "Create a plan",
        inputHint: "What should change?",
      }),
      $createTextNode(" the "),
      $createComposerChipNode({
        kind: "file",
        label: "index.ts",
        path: "/repo/src/index.ts",
        uri: "file:///repo/src/index.ts",
        detail: "./src/index.ts",
      }),
      $createTextNode(" and "),
      $createComposerChipNode({
        kind: "skill",
        label: "preact-sigma",
        path: "/repo/.agents/skills/preact-sigma/SKILL.md",
        uri: "file:///repo/.agents/skills/preact-sigma/SKILL.md",
        detail: "./.agents/skills/preact-sigma/SKILL.md",
        source: "local",
      }),
    )
    $getRoot().append(paragraph)
  })

  expect(serializeComposerEditorState(editorState)).toEqual([
    {
      type: "text",
      text: "Review /plan the ",
    },
    {
      type: "resource_link",
      name: "index.ts",
      uri: "file:///repo/src/index.ts",
      title: "index.ts",
      description: "./src/index.ts",
    },
    {
      type: "text",
      text: " and ",
    },
    {
      type: "resource_link",
      name: "preact-sigma",
      uri: "file:///repo/.agents/skills/preact-sigma/SKILL.md",
      title: "preact-sigma skill",
      description: "./.agents/skills/preact-sigma/SKILL.md",
    },
  ] satisfies ComposerPromptBlock[])
})

test("promptBlocksToTranscriptContent preserves resource links instead of flattening them", () => {
  expect(
    promptBlocksToTranscriptContent([
      {
        type: "text",
        text: "Review this file:",
      },
      {
        type: "resource_link",
        name: "index.ts",
        uri: "file:///repo/src/index.ts",
        title: "index.ts",
        description: "./src/index.ts",
      },
    ]),
  ).toEqual([
    {
      type: "text",
      text: "Review this file:",
    },
    {
      type: "resource_link",
      name: "index.ts",
      uri: "file:///repo/src/index.ts",
      title: "index.ts",
      description: "./src/index.ts",
    },
  ])
})

test("hasPromptContent requires non-whitespace text or at least one resource link", () => {
  expect(hasPromptContent([{ type: "text", text: "   " }])).toBe(false)
  expect(
    hasPromptContent([
      {
        type: "resource_link",
        name: "index.ts",
        uri: "file:///repo/src/index.ts",
      },
    ] satisfies ComposerPromptBlock[]),
  ).toBe(true)
})
