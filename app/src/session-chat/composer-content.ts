/** Shared session-composer serialization helpers for Lexical editor state and transcript content. */
import type { SessionPromptRequest } from "@goddard-ai/sdk"
import {
  $createLineBreakNode,
  $createParagraphNode,
  $createTextNode,
  $getRoot,
  $isElementNode,
  $isLineBreakNode,
  $isTextNode,
  type EditorState,
  type LexicalNode,
} from "lexical"

import {
  $createComposerChipNode,
  $isComposerChipNode,
  type ComposerChipData,
} from "./composer-chip-node.tsrx"

type ComposerPromptBlock = Exclude<SessionPromptRequest["prompt"], string>[number]
type ComposerPromptBlocks = Exclude<SessionPromptRequest["prompt"], string>

/** One transcript content block rendered inside a user, assistant, or system message. */
export type TranscriptContentBlock =
  | {
      type: "text"
      text: string
    }
  | {
      type: "resource_link"
      name: string
      uri: string
      title: string | null
      description: string | null
    }

type ComposerContentPart =
  | {
      type: "text"
      text: string
    }
  | {
      type: "chip"
      chip: ComposerChipData
    }

/** Coalesces adjacent text blocks while preserving non-text ACP content order. */
function mergeTextBlocks(blocks: ComposerPromptBlock[]) {
  const mergedBlocks: ComposerPromptBlock[] = []

  for (const block of blocks) {
    if (block.type === "text") {
      const previousBlock = mergedBlocks.at(-1)

      if (previousBlock?.type === "text") {
        previousBlock.text += block.text
        continue
      }
    }

    mergedBlocks.push(block)
  }

  return mergedBlocks.filter((block) => block.type !== "text" || block.text.length > 0)
}

/** Appends one lexical node subtree into the ordered composer content part list. */
function appendNodeParts(node: LexicalNode, parts: ComposerContentPart[]) {
  if ($isComposerChipNode(node)) {
    parts.push({
      type: "chip",
      chip: node.getChip(),
    })
    return
  }

  if ($isTextNode(node)) {
    parts.push({
      type: "text",
      text: node.getTextContent(),
    })
    return
  }

  if ($isLineBreakNode(node)) {
    parts.push({
      type: "text",
      text: "\n",
    })
    return
  }

  if (!$isElementNode(node)) {
    return
  }

  const children = node.getChildren()

  for (const child of children) {
    appendNodeParts(child, parts)
  }
}

/** Reads one Lexical editor state into an ordered mix of text and chip parts. */
function readComposerParts(editorState: EditorState) {
  const parts: ComposerContentPart[] = []

  editorState.read(() => {
    const children = $getRoot().getChildren()

    for (const [index, child] of children.entries()) {
      appendNodeParts(child, parts)

      if (index < children.length - 1) {
        parts.push({
          type: "text",
          text: "\n",
        })
      }
    }
  })

  return parts
}

/** Serializes one chip payload into the ACP block expected by the daemon prompt contract. */
function serializeChip(chip: ComposerChipData) {
  if (chip.kind === "slash_command") {
    return {
      type: "text",
      text: `/${chip.label}`,
    } satisfies ComposerPromptBlock
  }

  return {
    type: "resource_link",
    name: chip.label,
    uri: chip.uri,
    title: chip.kind === "skill" ? `${chip.label} skill` : chip.label,
    description: chip.detail,
  } satisfies ComposerPromptBlock
}

/** Returns true when one ACP prompt payload contains meaningful content to submit. */
export function hasPromptContent(blocks: readonly ComposerPromptBlock[]) {
  return blocks.some((block) => block.type !== "text" || block.text.trim().length > 0)
}

/** Serializes one Lexical editor state into ACP prompt blocks for `session/prompt`. */
export function serializeComposerEditorState(editorState: EditorState): ComposerPromptBlocks {
  const blocks: ComposerPromptBlock[] = []
  let textBuffer = ""

  for (const part of readComposerParts(editorState)) {
    if (part.type === "text") {
      textBuffer += part.text
      continue
    }

    if (part.chip.kind === "slash_command") {
      textBuffer += `/${part.chip.label}`
      continue
    }

    if (textBuffer.length > 0) {
      blocks.push({
        type: "text",
        text: textBuffer,
      })
      textBuffer = ""
    }

    blocks.push(serializeChip(part.chip))
  }

  if (textBuffer.length > 0) {
    blocks.push({
      type: "text",
      text: textBuffer,
    })
  }

  return mergeTextBlocks(blocks)
}

/** Converts ACP prompt blocks into transcript-friendly content blocks. */
export function promptBlocksToTranscriptContent(blocks: unknown): TranscriptContentBlock[] {
  if (!Array.isArray(blocks)) {
    return []
  }

  const content: TranscriptContentBlock[] = []

  for (const block of blocks) {
    if (typeof block !== "object" || block === null || !("type" in block)) {
      continue
    }

    if (block.type === "text" && typeof block.text === "string" && block.text.length > 0) {
      content.push({
        type: "text",
        text: block.text,
      })
      continue
    }

    if (
      block.type === "resource_link" &&
      typeof block.name === "string" &&
      typeof block.uri === "string"
    ) {
      content.push({
        type: "resource_link",
        name: block.name,
        uri: block.uri,
        title: typeof block.title === "string" ? block.title : null,
        description: typeof block.description === "string" ? block.description : null,
      })
    }
  }

  return content
}

/** Populates the active Lexical root with one trailing paragraph built from the given chip data. */
export function insertComposerChipIntoEditor(chip: ComposerChipData) {
  const paragraph = $createParagraphNode()
  paragraph.append($createComposerChipNode(chip), $createTextNode(" "))
  $getRoot().append(paragraph)
}

/** Inserts one line break into the current paragraph when the composer needs an explicit newline. */
export function insertComposerLineBreak() {
  const root = $getRoot()
  const children = root.getChildren()
  const lastParagraph = children.at(-1)

  if ($isElementNode(lastParagraph)) {
    lastParagraph.append($createLineBreakNode())
    return
  }

  const paragraph = $createParagraphNode()
  paragraph.append($createLineBreakNode())
  root.append(paragraph)
}
