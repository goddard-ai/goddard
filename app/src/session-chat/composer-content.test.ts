import type { SessionPromptRequest } from "@goddard-ai/sdk"
import { $createListItemNode, $createListNode, ListItemNode, ListNode } from "@lexical/list"
import {
  $createParagraphNode,
  $createTextNode,
  $getRoot,
  createEditor,
  type EditorState,
} from "lexical"
import { expect, test } from "vitest"

import { $createComposerChipNode, ComposerChipNode } from "./composer-chip-node.tsrx"
import {
  hasPromptContent,
  promptBlocksToTranscriptContent,
  serializeComposerEditorState,
} from "./composer-content.ts"

type ComposerPromptBlock = Exclude<SessionPromptRequest["prompt"], string>[number]

function buildEditorState(build: () => void): EditorState {
  const editor = createEditor({
    nodes: [ComposerChipNode, ListNode, ListItemNode],
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
      $createTextNode(" and "),
      $createComposerChipNode({
        kind: "link",
        label: "https://example.com/docs",
        uri: "https://example.com/docs",
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
    {
      type: "text",
      text: " and ",
    },
    {
      type: "resource_link",
      name: "https://example.com/docs",
      uri: "https://example.com/docs",
      title: "https://example.com/docs",
      description: "https://example.com/docs",
    },
  ] satisfies ComposerPromptBlock[])
})

test("serializeComposerEditorState preserves rendered list markers", () => {
  const editorState = buildEditorState(() => {
    const bulletList = $createListNode("bullet")
    const firstBullet = $createListItemNode()
    const secondBullet = $createListItemNode()
    firstBullet.append($createTextNode("first"))
    secondBullet.append($createTextNode("second"))
    bulletList.append(firstBullet, secondBullet)

    const numberedList = $createListNode("number", 3)
    const firstNumbered = $createListItemNode()
    const secondNumbered = $createListItemNode()
    firstNumbered.append($createTextNode("third"))
    secondNumbered.append($createTextNode("fourth"))
    numberedList.append(firstNumbered, secondNumbered)

    $getRoot().append(bulletList, numberedList)
  })

  expect(serializeComposerEditorState(editorState)).toEqual([
    {
      type: "text",
      text: "- first\n- second\n3. third\n4. fourth",
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
