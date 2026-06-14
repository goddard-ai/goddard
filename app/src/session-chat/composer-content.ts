/** Shared session-composer serialization helpers for Lexical editor state and transcript content. */
import type { SessionPromptRequest } from "@goddard-ai/sdk"
import { $isListItemNode, $isListNode } from "@lexical/list"
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
  chipTextFallback,
  type ComposerChipData,
  type ComposerChipNode,
} from "./composer-chip-node.tsrx"
import { $isComposerShellPromptNode } from "./composer-shell-prompt-node.tsrx"

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
      type: "shell_prompt"
    }
  | {
      type: "chip"
      chip: ComposerChipData
    }

type ComposerLineSegment =
  | {
      type: "text"
      text: string
    }
  | {
      type: "chip"
      chip: ComposerChipData
    }

type ComposerLine = {
  shellPrompt: boolean
  segments: ComposerLineSegment[]
}

const LIST_INDENT = "    "

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

function appendTextPart(parts: ComposerContentPart[], text: string) {
  parts.push({
    type: "text",
    text,
  })
}

function appendListParts(node: LexicalNode, parts: ComposerContentPart[], depth: number) {
  if (!$isListNode(node)) {
    return
  }

  let itemIndex = 0

  for (const child of node.getChildren()) {
    if (!$isListItemNode(child)) {
      appendNodeParts(child, parts)
      continue
    }

    const firstChild = child.getFirstChild()

    if (child.getChildrenSize() === 1 && $isListNode(firstChild)) {
      if (itemIndex > 0) {
        appendTextPart(parts, "\n")
      }

      appendListParts(firstChild, parts, depth + 1)
      continue
    }

    if (itemIndex > 0) {
      appendTextPart(parts, "\n")
    }

    appendTextPart(
      parts,
      `${LIST_INDENT.repeat(depth)}${
        node.getListType() === "number" ? `${node.getStart() + itemIndex}. ` : "- "
      }`,
    )

    for (const itemChild of child.getChildren()) {
      if ($isListNode(itemChild)) {
        appendTextPart(parts, "\n")
        appendListParts(itemChild, parts, depth + 1)
        continue
      }

      appendNodeParts(itemChild, parts)
    }

    itemIndex++
  }
}

/** Appends one lexical node subtree into the ordered composer content part list. */
function appendNodeParts(node: LexicalNode, parts: ComposerContentPart[]) {
  if ($isComposerChipNode(node)) {
    const chipNode = node as ComposerChipNode

    parts.push({
      type: "chip",
      chip: chipNode.getChip(),
    })
    return
  }

  if ($isComposerShellPromptNode(node)) {
    parts.push({
      type: "shell_prompt",
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

  if ($isListNode(node)) {
    appendListParts(node, parts, 0)
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

function buildComposerLines(parts: ComposerContentPart[]) {
  const lines: ComposerLine[] = [{ shellPrompt: false, segments: [] }]

  function currentLine() {
    return lines[lines.length - 1]!
  }

  function pushText(text: string) {
    const segments = currentLine().segments
    const previousSegment = segments.at(-1)

    if (previousSegment?.type === "text") {
      previousSegment.text += text
      return
    }

    segments.push({
      type: "text",
      text,
    })
  }

  for (const part of parts) {
    if (part.type === "text") {
      const fragments = part.text.split("\n")

      for (const [index, fragment] of fragments.entries()) {
        if (fragment.length > 0) {
          pushText(fragment)
        }

        if (index < fragments.length - 1) {
          lines.push({
            shellPrompt: false,
            segments: [],
          })
        }
      }

      continue
    }

    if (part.type === "shell_prompt") {
      const line = currentLine()

      if (!line.shellPrompt && line.segments.length === 0) {
        line.shellPrompt = true
        continue
      }

      pushText("$ ")
      continue
    }

    currentLine().segments.push(part)
  }

  return lines
}

function serializeRegularLine(
  line: ComposerLine,
  appendText: (text: string) => void,
  flushText: () => void,
  blocks: ComposerPromptBlock[],
) {
  for (const segment of line.segments) {
    if (segment.type === "text") {
      appendText(segment.text)
      continue
    }

    if (segment.chip.kind === "slash_command") {
      appendText(`/${segment.chip.label}`)
      continue
    }

    flushText()
    blocks.push(serializeChip(segment.chip))
  }
}

function serializeShellLine(line: ComposerLine) {
  return line.segments
    .map((segment) => (segment.type === "text" ? segment.text : chipTextFallback(segment.chip)))
    .join("")
}

function trimEmptyShellLines(lines: string[]) {
  let start = 0
  let end = lines.length

  while (start < end && lines[start]!.trim().length === 0) {
    start += 1
  }

  while (end > start && lines[end - 1]!.trim().length === 0) {
    end -= 1
  }

  return lines.slice(start, end)
}

/** Serializes one chip payload into the ACP block expected by the daemon prompt contract. */
function serializeChip(chip: ComposerChipData) {
  if (chip.kind === "slash_command") {
    return {
      type: "text",
      text: `/${chip.label}`,
    } satisfies ComposerPromptBlock
  }

  if (chip.kind === "link") {
    return {
      type: "resource_link",
      name: chip.label,
      uri: chip.uri,
      title: chip.label,
      description: chip.uri,
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

  function appendText(text: string) {
    textBuffer += text
  }

  function flushText() {
    if (textBuffer.length === 0) {
      return
    }

    blocks.push({
      type: "text",
      text: textBuffer,
    })
    textBuffer = ""
  }

  const lines = buildComposerLines(readComposerParts(editorState))

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index]!

    if (!line.shellPrompt) {
      serializeRegularLine(line, appendText, flushText, blocks)

      if (index < lines.length - 1) {
        appendText("\n")
      }

      continue
    }

    const shellLines: string[] = []

    while (index < lines.length && lines[index]!.shellPrompt) {
      shellLines.push(serializeShellLine(lines[index]!))
      index += 1
    }

    const normalizedShellLines = trimEmptyShellLines(shellLines)

    if (normalizedShellLines.length > 0) {
      flushText()
      blocks.push({
        type: "text",
        text: `\`\`\`shell\n${normalizedShellLines.join("\n")}\n\`\`\``,
      })
    }

    if (index < lines.length) {
      appendText("\n")
    }

    index -= 1
  }

  flushText()
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
