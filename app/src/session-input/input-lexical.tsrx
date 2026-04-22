import { useLexicalComposerContext } from "@lexical/react/LexicalComposerContext"
import { HistoryPlugin } from "@lexical/react/LexicalHistoryPlugin"
import { OnChangePlugin } from "@lexical/react/LexicalOnChangePlugin"
import type { LexicalEditor, LexicalNode, NodeKey } from "lexical"
import {
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
  COMMAND_PRIORITY_HIGH,
  KEY_ARROW_DOWN_COMMAND,
  KEY_ARROW_UP_COMMAND,
  KEY_ENTER_COMMAND,
  KEY_ESCAPE_COMMAND,
} from "lexical"
import { useEffect } from "preact/hooks"

import {
  $createComposerChipNode,
  ComposerChipNode,
  type ComposerChipData,
} from "~/session-chat/composer-chip-node.tsrx"
import { serializeComposerEditorState } from "~/session-chat/composer-content.ts"
import type {
  SessionInputPromptBlocks,
  SessionInputSuggestion,
  SessionInputTrigger,
} from "./input.tsrx"

export type SessionInputMenuState = {
  trigger: SessionInputTrigger;
  query: string;
  nodeKey: NodeKey;
  startOffset: number;
  endOffset: number;
  anchorLeft: number;
  anchorTop: number;
}

export const sessionInputInitialConfig = {
  namespace: "session-input",
  nodes: [ComposerChipNode],
  onError(error: Error) {
    throw error
  },
}

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

function detectSessionInputMenuState() {
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
    trigger: match[1] === "@"
      ? "at"
      : match[1] === "$"
        ? "dollar"
        : "slash",
    query: match[2] ?? "",
    nodeKey: anchorNode.getKey(),
    startOffset: anchorOffset - triggerToken.length,
    endOffset: anchorOffset,
  }

  return menuState
}

function getCaretAnchorPosition(editor: LexicalEditor) {
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

export function clearSessionInputEditor(editor: LexicalEditor) {
  editor.update(() => {
    const root = $getRoot()
    root.clear()
    root.append($createParagraphNode())
  }, { discrete: true })
}

export function insertSessionInputSuggestion(
  editor: LexicalEditor,
  menu: SessionInputMenuState,
  suggestion: SessionInputSuggestion,
) {
  const chip = suggestionToChip(suggestion)

  editor.update(() => {
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
  }, { discrete: true })
}

export component SessionInputPlugins(props: {
  menu: SessionInputMenuState | null;
  onEditorReady: (editor: LexicalEditor) => void;
  onDismissMenu: () => void;
  onHighlightNext: () => void;
  onHighlightPrevious: () => void;
  onMenuChange: (menu: SessionInputMenuState | null) => void;
  onPromptBlocksChange: (blocks: SessionInputPromptBlocks) => void;
  onEscape?: () => void;
  onSubmit: () => void;
  onAcceptMenu: () => void;
}) {
  const [editor] = useLexicalComposerContext()

  useEffect(() => {
    props.onEditorReady(editor)
  }, [editor, props])

  useEffect(() => {
    return editor.registerUpdateListener(({ editorState }) => {
      const detectedMenu = editorState.read(() => detectSessionInputMenuState())

      if (!detectedMenu) {
        props.onMenuChange(null)
        return
      }

      props.onMenuChange(
        {
          trigger: detectedMenu.trigger,
          query: detectedMenu.query,
          nodeKey: detectedMenu.nodeKey,
          startOffset: detectedMenu.startOffset,
          endOffset: detectedMenu.endOffset,
          ...getCaretAnchorPosition(editor),
        },
      )
    })
  }, [editor, props])

  useEffect(() => {
    const unregisterEnter = editor.registerCommand(
      KEY_ENTER_COMMAND,
      (event) => {
        if (props.menu) {
          event?.preventDefault()
          props.onAcceptMenu()
          return true
        }

        if (event?.shiftKey) {
          return false
        }

        event?.preventDefault()
        props.onSubmit()
        return true
      },
      COMMAND_PRIORITY_HIGH,
    )
    const unregisterArrowDown = editor.registerCommand(
      KEY_ARROW_DOWN_COMMAND,
      (event) => {
        if (!props.menu) {
          return false
        }

        event?.preventDefault()
        props.onHighlightNext()
        return true
      },
      COMMAND_PRIORITY_HIGH,
    )
    const unregisterArrowUp = editor.registerCommand(
      KEY_ARROW_UP_COMMAND,
      (event) => {
        if (!props.menu) {
          return false
        }

        event?.preventDefault()
        props.onHighlightPrevious()
        return true
      },
      COMMAND_PRIORITY_HIGH,
    )
    const unregisterEscape = editor.registerCommand(
      KEY_ESCAPE_COMMAND,
      (event) => {
        if (!props.menu) {
          if (!props.onEscape) {
            return false
          }

          event?.preventDefault()
          props.onEscape()
          return true
        }

        event?.preventDefault()
        props.onDismissMenu()
        return true
      },
      COMMAND_PRIORITY_HIGH,
    )

    return () => {
      unregisterEnter()
      unregisterArrowDown()
      unregisterArrowUp()
      unregisterEscape()
    }
  }, [editor, props])

  <HistoryPlugin />
  <OnChangePlugin
    ignoreSelectionChange={true}
    onInput={(editorState) => {
      props.onPromptBlocksChange(serializeComposerEditorState(editorState))
    }}
  />
}
