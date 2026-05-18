/** Lexical document mutation helpers for the session input composer. */
import {
  $createLineBreakNode,
  $createParagraphNode,
  $createTextNode,
  $getNodeByKey,
  $getRoot,
  $insertNodes,
  $isTextNode,
  type LexicalEditor,
} from "lexical"

import {
  $createComposerChipNode,
  type ComposerChipData,
} from "~/session-chat/composer-chip-node.tsrx"
import type { SessionInputMenuState } from "./input-menu-detection.ts"
import type { SessionInputPromptBlocks, SessionInputSuggestion } from "./input.tsrx"

function suggestionToChip(suggestion: SessionInputSuggestion): ComposerChipData {
  if (suggestion.type === "slash_command") {
    return {
      kind: "slash_command",
      label: suggestion.name,
      description: suggestion.description,
      inputHint: suggestion.inputHint ?? null,
    }
  }

  if (suggestion.type === "skill") {
    return {
      kind: "skill",
      label: suggestion.label,
      path: suggestion.path,
      uri: suggestion.uri,
      detail: suggestion.detail,
      source: suggestion.source,
    }
  }

  return {
    kind: suggestion.type,
    label: suggestion.label,
    path: suggestion.path,
    uri: suggestion.uri,
    detail: suggestion.detail,
  }
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
          for (const [index, textSegment] of block.text.split("\n").entries()) {
            if (index > 0) {
              paragraph.append($createLineBreakNode())
            }

            if (textSegment.length > 0) {
              paragraph.append($createTextNode(textSegment))
            }
          }

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
