/** Lexical document mutation helpers for the session input composer. */
import {
  $createLineBreakNode,
  $createParagraphNode,
  $createTextNode,
  $getNodeByKey,
  $getRoot,
  $getSelection,
  $insertNodes,
  $isElementNode,
  $isLineBreakNode,
  $isRangeSelection,
  $isTextNode,
  TextNode,
  type LexicalEditor,
  type LexicalNode,
} from "lexical"

import {
  $createComposerChipNode,
  $isComposerChipNode,
  type ComposerChipData,
} from "~/session-chat/composer-chip-node.tsrx"
import {
  $createComposerShellPromptNode,
  ComposerShellPromptNode,
} from "~/session-chat/composer-shell-prompt-node.tsrx"
import type { SessionInputMenuState } from "./input-menu-detection.ts"
import type { SessionInputPromptBlocks, SessionInputSuggestion } from "./input.tsrx"

function isWebLinkUri(uri: string) {
  try {
    const url = new URL(uri)
    return url.protocol === "http:" || url.protocol === "https:"
  } catch {
    return false
  }
}

function removeComposerChipNode(node: LexicalNode | null | undefined) {
  if (!$isComposerChipNode(node)) {
    return false
  }

  node.remove()
  return true
}

export function deleteSessionInputChipBeforeCaret() {
  const selection = $getSelection()

  if (!$isRangeSelection(selection) || !selection.isCollapsed()) {
    return false
  }

  const anchor = selection.anchor
  const anchorNode = anchor.getNode()

  if ($isTextNode(anchorNode)) {
    if (anchor.offset !== 0) {
      return false
    }

    if (!removeComposerChipNode(anchorNode.getPreviousSibling())) {
      return false
    }

    anchorNode.select(0, 0)
    return true
  }

  if (!$isElementNode(anchorNode) || anchor.offset === 0) {
    return false
  }

  const chipIndex = anchor.offset - 1

  if (!removeComposerChipNode(anchorNode.getChildAtIndex(chipIndex))) {
    return false
  }

  anchorNode.select(chipIndex, chipIndex)
  return true
}

function suggestionToChip(suggestion: SessionInputSuggestion): ComposerChipData {
  switch (suggestion.type) {
    case "slash_command":
      return {
        kind: "slash_command",
        label: suggestion.name,
        description: suggestion.description,
        inputHint: suggestion.inputHint ?? null,
      }
    case "skill":
      return {
        kind: "skill",
        label: suggestion.label,
        path: suggestion.path,
        uri: suggestion.uri,
        detail: suggestion.detail,
        source: suggestion.source,
      }
    case "file":
    case "folder":
      return {
        kind: suggestion.type,
        label: suggestion.label,
        path: suggestion.path,
        uri: suggestion.uri,
        detail: suggestion.detail,
      }
  }

  suggestion satisfies never
}

export function normalizeSessionInputShellPromptTextNode(node: TextNode) {
  const text = node.getTextContent()

  if (!text.startsWith("$ ")) {
    return
  }

  const previousSibling = node.getPreviousSibling()

  if (previousSibling !== null && !$isLineBreakNode(previousSibling)) {
    return
  }

  node.spliceText(0, 2, "", false)
  node.insertBefore($createComposerShellPromptNode())
}

export function normalizeSessionInputShellPromptNode(node: ComposerShellPromptNode) {
  const previousSibling = node.getPreviousSibling()

  if (previousSibling === null || $isLineBreakNode(previousSibling)) {
    return
  }

  node.replace($createTextNode("$ "))
}

export function clearSessionInputEditor(editor: LexicalEditor) {
  editor.update(
    () => {
      const root = $getRoot()
      root.clear()
      root.append($createParagraphNode())
    },
    { discrete: true },
  )
}

function createComposerChipFromPromptBlock(block: SessionInputPromptBlocks[number]) {
  if (block.type !== "resource_link") {
    return null
  }

  if (isWebLinkUri(block.uri)) {
    const chip: ComposerChipData = {
      kind: "link",
      label: block.name,
      uri: block.uri,
    }

    return chip
  }

  if (block.title?.endsWith(" skill")) {
    const chip: ComposerChipData = {
      kind: "skill",
      label: block.name,
      path: block.uri,
      uri: block.uri,
      detail: block.description ?? block.uri,
      source: "local",
    }

    return chip
  }

  const chip: ComposerChipData = {
    kind: "file",
    label: block.name,
    path: block.uri,
    uri: block.uri,
    detail: block.description ?? block.uri,
  }

  return chip
}

function appendPromptTextToParagraph(
  paragraph: ReturnType<typeof $createParagraphNode>,
  text: string,
) {
  let insideShellBlock = false

  // Round-trip shell drafts through the lexical shell prompt instead of showing raw fences.
  for (const line of text.split("\n")) {
    if (!insideShellBlock && line === "```shell") {
      insideShellBlock = true
      continue
    }

    if (insideShellBlock && line === "```") {
      insideShellBlock = false
      continue
    }

    if (paragraph.getChildrenSize() > 0) {
      paragraph.append($createLineBreakNode())
    }

    if (insideShellBlock) {
      paragraph.append($createComposerShellPromptNode())

      if (line.length > 0) {
        paragraph.append($createTextNode(line))
      }

      continue
    }

    if (line.length > 0) {
      paragraph.append($createTextNode(line))
    }
  }
}

/** Rehydrates one saved prompt block draft so a dismissed launch dialog can show it again. */
export function setSessionInputEditorPrompt(
  editor: LexicalEditor,
  blocks: SessionInputPromptBlocks,
) {
  editor.update(
    () => {
      const root = $getRoot()
      const paragraph = $createParagraphNode()

      root.clear()
      root.append(paragraph)

      for (const block of blocks) {
        if (block.type === "text") {
          appendPromptTextToParagraph(paragraph, block.text)
          continue
        }

        const chip = createComposerChipFromPromptBlock(block)

        if (chip) {
          paragraph.append($createComposerChipNode(chip), $createTextNode(" "))
        }
      }
    },
    { discrete: true },
  )
}

export function insertSessionInputSuggestion(
  editor: LexicalEditor,
  menu: SessionInputMenuState,
  suggestion: SessionInputSuggestion,
) {
  const chip = suggestionToChip(suggestion)

  editor.update(
    () => {
      const trailingSpace = $createTextNode(" ")
      const targetNode = $getNodeByKey(menu.nodeKey)

      if ($isTextNode(targetNode)) {
        targetNode.spliceText(menu.startOffset, menu.endOffset - menu.startOffset, "", false)
        targetNode.select(menu.startOffset, menu.startOffset)
        $insertNodes([$createComposerChipNode(chip), trailingSpace])
        trailingSpace.selectEnd()
        return
      }

      const paragraph = $createParagraphNode()
      paragraph.append($createComposerChipNode(chip), trailingSpace)
      $getRoot().append(paragraph)
      trailingSpace.selectEnd()
    },
    { discrete: true },
  )
}

export function insertSessionInputWebLink(
  editor: LexicalEditor,
  link: { label: string; uri: string },
) {
  editor.update(
    () => {
      const trailingSpace = $createTextNode(" ")
      $insertNodes([
        $createComposerChipNode({
          kind: "link",
          label: link.label,
          uri: link.uri,
        }),
        trailingSpace,
      ])
      trailingSpace.selectEnd()
    },
    { discrete: true },
  )
}
