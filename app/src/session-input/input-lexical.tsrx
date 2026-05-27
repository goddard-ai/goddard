import { ListItemNode, ListNode } from "@lexical/list"
import { ORDERED_LIST, UNORDERED_LIST } from "@lexical/markdown"
import { useLexicalComposerContext } from "@lexical/react/LexicalComposerContext"
import { HistoryPlugin } from "@lexical/react/LexicalHistoryPlugin"
import { ListPlugin } from "@lexical/react/LexicalListPlugin"
import { MarkdownShortcutPlugin } from "@lexical/react/LexicalMarkdownShortcutPlugin"
import { OnChangePlugin } from "@lexical/react/LexicalOnChangePlugin"
import type { LexicalEditor } from "lexical"
import {
  COMMAND_PRIORITY_HIGH,
  $getSelection,
  $isRangeSelection,
  KEY_ARROW_DOWN_COMMAND,
  KEY_ARROW_UP_COMMAND,
  KEY_ENTER_COMMAND,
  KEY_ESCAPE_COMMAND,
  PASTE_COMMAND,
} from "lexical"
import { useEffect } from "preact/hooks"

import { ComposerChipNode } from "~/session-chat/composer-chip-node.tsrx"
import { serializeComposerEditorState } from "~/session-chat/composer-content.ts"
import { insertSessionInputWebLink } from "./input-editor-content.ts"
import {
  detectSessionInputMenuState,
  getSessionInputMenuAnchorPosition,
  type SessionInputMenuState,
} from "./input-menu-detection.ts"
import { isSessionInputCaretAtPromptEnd } from "./input-selection.ts"
import type { SessionInputPromptBlocks } from "./input.tsrx"

const sessionInputMarkdownTransformers = [UNORDERED_LIST, ORDERED_LIST]

export const sessionInputInitialConfig = {
  namespace: "session-input",
  nodes: [ComposerChipNode, ListNode, ListItemNode],
  onError(error: Error) {
    throw error
  },
}

function readPastedWebLink(event: Event) {
  if (typeof ClipboardEvent === "undefined" || !(event instanceof ClipboardEvent)) {
    return null
  }

  const text = event.clipboardData?.getData("text/plain").trim()

  if (!text || /\s/.test(text)) {
    return null
  }

  try {
    const url = new URL(text)

    if (url.protocol !== "http:" && url.protocol !== "https:") {
      return null
    }

    return {
      label: text,
      uri: url.href,
    }
  } catch {
    return null
  }
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
  onHistoryNext?: () => boolean;
  onHistoryPrevious?: () => boolean;
  onSubmit: () => void;
  onAcceptMenu: () => void;
}) {
  const [editor] = useLexicalComposerContext()

  useEffect(() => {
    props.onEditorReady(editor)
  }, [editor])

  useEffect(() => {
    return editor.registerUpdateListener(({ editorState }) => {
      const detectedMenu = editorState.read(() => detectSessionInputMenuState())

      if (!detectedMenu) {
        props.onMenuChange(null)
        return
      }

      props.onMenuChange({
        trigger: detectedMenu.trigger,
        query: detectedMenu.query,
        nodeKey: detectedMenu.nodeKey,
        startOffset: detectedMenu.startOffset,
        endOffset: detectedMenu.endOffset,
        ...getSessionInputMenuAnchorPosition(editor),
      })
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

        if (!event?.metaKey && !event?.ctrlKey && !isSessionInputCaretAtPromptEnd()) {
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
          if (props.onHistoryNext?.()) {
            event?.preventDefault()
            return true
          }

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
          if (isSelectionAtComposerStart() && props.onHistoryPrevious?.()) {
            event?.preventDefault()
            return true
          }

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
    const unregisterPaste = editor.registerCommand(
      PASTE_COMMAND,
      (event) => {
        const link = readPastedWebLink(event)

        if (!link) {
          return false
        }

        event.preventDefault()
        insertSessionInputWebLink(editor, link)
        return true
      },
      COMMAND_PRIORITY_HIGH,
    )

    return () => {
      unregisterEnter()
      unregisterArrowDown()
      unregisterArrowUp()
      unregisterEscape()
      unregisterPaste()
    }
  }, [editor, props])

  <HistoryPlugin />
  <ListPlugin />
  <MarkdownShortcutPlugin transformers={sessionInputMarkdownTransformers} />
  <OnChangePlugin
    ignoreSelectionChange={true}
    onChange={(editorState) => {
      props.onPromptBlocksChange(serializeComposerEditorState(editorState))
    }}
  />
}

function isSelectionAtComposerStart() {
  const selection = $getSelection()

  if (!$isRangeSelection(selection) || !selection.isCollapsed()) {
    return false
  }

  const anchor = selection.anchor

  if (anchor.offset !== 0) {
    return false
  }

  let node = anchor.getNode()

  while (node.getParent() !== null) {
    if (node.getPreviousSibling() !== null) {
      return false
    }

    node = node.getParentOrThrow()
  }

  return true
}
