import { useLexicalComposerContext } from "@lexical/react/LexicalComposerContext"
import { HistoryPlugin } from "@lexical/react/LexicalHistoryPlugin"
import { OnChangePlugin } from "@lexical/react/LexicalOnChangePlugin"
import type { LexicalEditor } from "lexical"
import {
  COMMAND_PRIORITY_HIGH,
  KEY_ARROW_DOWN_COMMAND,
  KEY_ARROW_UP_COMMAND,
  KEY_ENTER_COMMAND,
  KEY_ESCAPE_COMMAND,
} from "lexical"
import { useEffect } from "preact/hooks"

import { ComposerChipNode } from "~/session-chat/composer-chip-node.tsrx"
import { serializeComposerEditorState } from "~/session-chat/composer-content.ts"
import {
  detectSessionInputMenuState,
  getSessionInputMenuAnchorPosition,
  type SessionInputMenuState,
} from "./input-menu-detection.ts"
import type { SessionInputPromptBlocks } from "./input.tsrx"

export const sessionInputInitialConfig = {
  namespace: "session-input",
  nodes: [ComposerChipNode],
  onError(error: Error) {
    throw error
  },
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
    onChange={(editorState) => {
      props.onPromptBlocksChange(serializeComposerEditorState(editorState))
    }}
  />
}
