/** Lexical selection helpers for detecting and positioning session input suggestion menus. */
import {
  $getRoot,
  $getSelection,
  $isElementNode,
  $isLineBreakNode,
  $isRangeSelection,
  $isTextNode,
  type LexicalEditor,
  type LexicalNode,
  type NodeKey,
} from "lexical"

import type { SessionInputTrigger } from "./input.tsrx"

export type SessionInputMenuState = {
  trigger: SessionInputTrigger
  query: string
  nodeKey: NodeKey
  startOffset: number
  endOffset: number
  anchorLeft: number
  anchorTop: number
}

function readTextBeforeCaret(nodeKey: NodeKey, offset: number) {
  let foundCursor = false
  let text = ""

  function appendNodeText(node: LexicalNode) {
    if (foundCursor) {
      return
    }

    if ($isTextNode(node)) {
      if (node.getKey() === nodeKey) {
        text += node.getTextContent().slice(0, offset)
        foundCursor = true
        return
      }

      text += node.getTextContent()
      return
    }

    if ($isLineBreakNode(node)) {
      text += "\n"
      return
    }

    if (!$isElementNode(node)) {
      text += node.getTextContent()
      return
    }

    for (const child of node.getChildren()) {
      appendNodeText(child)

      if (foundCursor) {
        return
      }
    }
  }

  const topLevelChildren = $getRoot().getChildren()

  for (const [index, child] of topLevelChildren.entries()) {
    appendNodeText(child)

    if (foundCursor) {
      return text
    }

    if (index < topLevelChildren.length - 1) {
      text += "\n"
    }
  }

  return null
}

export function detectSessionInputMenuState() {
  const selection = $getSelection()

  if (!$isRangeSelection(selection) || !selection.isCollapsed()) {
    return null
  }

  const anchorNode = selection.anchor.getNode()

  if (!$isTextNode(anchorNode)) {
    return null
  }

  const anchorOffset = selection.anchor.offset

  if (anchorOffset === 0) {
    return null
  }

  const textBeforeCaretInNode = anchorNode.getTextContent().slice(0, anchorOffset)
  const match = /(?:^|\s)([@$/])([^\s]*)$/.exec(textBeforeCaretInNode)

  if (!match) {
    return null
  }

  const triggerToken = `${match[1]}${match[2] ?? ""}`
  const fullTextBeforeCaret = readTextBeforeCaret(anchorNode.getKey(), anchorOffset)

  if (!fullTextBeforeCaret || !fullTextBeforeCaret.endsWith(triggerToken)) {
    return null
  }

  if (match[1] === "/" && fullTextBeforeCaret.slice(0, -triggerToken.length).trim().length > 0) {
    return null
  }

  const menuState: Omit<SessionInputMenuState, "anchorLeft" | "anchorTop"> = {
    trigger: match[1] === "@" ? "at" : match[1] === "$" ? "dollar" : "slash",
    query: match[2] ?? "",
    nodeKey: anchorNode.getKey(),
    startOffset: anchorOffset - triggerToken.length,
    endOffset: anchorOffset,
  }

  return menuState
}

export function getSessionInputMenuAnchorPosition(editor: LexicalEditor) {
  const selection = window.getSelection()
  const rootElement = editor.getRootElement()

  if (!selection || selection.rangeCount === 0) {
    const fallbackRect = rootElement?.getBoundingClientRect()

    if (!fallbackRect) {
      return {
        anchorLeft: 16,
        anchorTop: 16,
      }
    }

    return {
      anchorLeft: Math.max(16, Math.min(fallbackRect.left + 16, window.innerWidth - 376)),
      anchorTop: fallbackRect.top + 56,
    }
  }

  const range = selection.getRangeAt(0).cloneRange()
  range.collapse(false)
  const rangeRect = range.getBoundingClientRect()
  const fallbackRect = rootElement?.getBoundingClientRect()
  const left = rangeRect.left || fallbackRect?.left || 16
  const top = rangeRect.bottom || fallbackRect?.top || 16

  return {
    anchorLeft: Math.max(16, Math.min(left, window.innerWidth - 376)),
    anchorTop: Math.max(16, Math.min(top + 12, window.innerHeight - 32)),
  }
}
