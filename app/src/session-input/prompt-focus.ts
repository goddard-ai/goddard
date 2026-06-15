import {
  $createRangeSelectionFromDom,
  $getSelection,
  $setSelection,
  type BaseSelection,
  type LexicalEditor,
} from "lexical"

type PromptFocusRestore = () => void
type PromptFocusSnapshot = {
  lexicalSelection: BaseSelection | null
  domRange: Range | null
}

const sessionInputEditors = new Set<LexicalEditor>()

export function registerSessionInputPromptFocus(editor: LexicalEditor) {
  sessionInputEditors.add(editor)

  return () => {
    sessionInputEditors.delete(editor)
  }
}

export function captureFocusedSessionInputPrompt(): PromptFocusRestore | null {
  const activeElement = document.activeElement

  for (const editor of sessionInputEditors) {
    const rootElement = editor.getRootElement()

    if (!rootElement || !(activeElement instanceof Node) || !rootElement.contains(activeElement)) {
      continue
    }

    const snapshot = capturePromptFocusSnapshot(editor, rootElement)

    return () => {
      queueMicrotask(() => {
        restorePromptFocus(editor, snapshot)
      })
    }
  }

  return null
}

function capturePromptFocusSnapshot(
  editor: LexicalEditor,
  rootElement: HTMLElement,
): PromptFocusSnapshot {
  const selection = window.getSelection()
  const domRange =
    selection &&
    selection.rangeCount > 0 &&
    rootElement.contains(selection.anchorNode) &&
    rootElement.contains(selection.focusNode)
      ? selection.getRangeAt(0).cloneRange()
      : null

  return {
    lexicalSelection: editor.getEditorState().read(() => $getSelection()?.clone() ?? null),
    domRange,
  }
}

function restorePromptFocus(editor: LexicalEditor, snapshot: PromptFocusSnapshot) {
  const restoreDomRange = () => {
    if (!snapshot.domRange) {
      return
    }

    const selection = window.getSelection()

    if (!selection) {
      return
    }

    selection.removeAllRanges()
    selection.addRange(snapshot.domRange)
  }

  editor.focus(() => {
    if (snapshot.domRange) {
      editor.update(() => {
        restoreDomRange()
        $setSelection($createRangeSelectionFromDom(window.getSelection(), editor))
      })
      return
    }

    if (snapshot.lexicalSelection) {
      editor.update(() => {
        $setSelection(snapshot.lexicalSelection?.clone() ?? null)
      })
    }
  })
}
