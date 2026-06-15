import { $isListItemNode } from "@lexical/list"
import {
  $getSelection,
  $isElementNode,
  $isRangeSelection,
  $isTextNode,
  type LexicalNode,
} from "lexical"

function hasFollowingNode(node: LexicalNode) {
  let currentNode: LexicalNode | null = node

  while (currentNode) {
    if (currentNode.getNextSibling()) {
      return true
    }

    currentNode = currentNode.getParent()
  }

  return false
}

/** Returns true when the current selection is one collapsed caret at the prompt end. */
export function isSessionInputCaretAtPromptEnd() {
  const selection = $getSelection()

  if (!$isRangeSelection(selection) || !selection.isCollapsed()) {
    return false
  }

  const focus = selection.focus
  const focusNode = focus.getNode()

  if ($isTextNode(focusNode)) {
    return focus.offset === focusNode.getTextContentSize() && !hasFollowingNode(focusNode)
  }

  if (!$isElementNode(focusNode)) {
    return false
  }

  return focus.offset === focusNode.getChildrenSize() && !hasFollowingNode(focusNode)
}

export function isSessionInputCaretInsideList() {
  const selection = $getSelection()

  if (!$isRangeSelection(selection) || !selection.isCollapsed()) {
    return false
  }

  let currentNode: LexicalNode | null = selection.focus.getNode()

  while (currentNode) {
    if ($isListItemNode(currentNode)) {
      return true
    }

    currentNode = currentNode.getParent()
  }

  return false
}
