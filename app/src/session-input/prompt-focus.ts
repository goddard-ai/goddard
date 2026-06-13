import { $getSelection, $setSelection, type BaseSelection, type LexicalEditor } from "lexical"

type PromptFocusRestore = () => void

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

    const selection = editor.getEditorState().read(() => $getSelection()?.clone() ?? null)

    return () => {
      queueMicrotask(() => {
        restorePromptSelection(editor, selection)
        editor.focus()
      })
    }
  }

  return null
}

function restorePromptSelection(editor: LexicalEditor, selection: BaseSelection | null) {
  if (!selection) {
    return
  }

  editor.update(() => {
    $setSelection(selection.clone())
  })
}
