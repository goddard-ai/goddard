import type { SessionComposerSuggestionsResponse, SessionPromptRequest } from "@goddard-ai/sdk"
import { LexicalComposer } from "@lexical/react/LexicalComposer"
import { ContentEditable } from "@lexical/react/LexicalContentEditable"
import { LexicalErrorBoundary } from "@lexical/react/LexicalErrorBoundary"
import { PlainTextPlugin } from "@lexical/react/LexicalPlainTextPlugin"
import type { LexicalEditor } from "lexical"
import { SendHorizontal } from "lucide-react"
import { useListener } from "preact-sigma"
import { useEffect, useRef, useState } from "preact/hooks"

import { hasPromptContent } from "~/session-chat/composer-content.ts"
import {
  clearSessionInputEditor,
  insertSessionInputSuggestion,
  SessionInputPlugins,
  sessionInputInitialConfig,
  type SessionInputMenuState,
} from "./input-lexical.tsrx"
import { SessionInputMenu } from "./input-menu.tsrx"

export type SessionInputSuggestion = SessionComposerSuggestionsResponse["suggestions"][number]
export type SessionInputPromptBlocks = Exclude<SessionPromptRequest["prompt"], string>
export type SessionInputTrigger = "at" | "dollar" | "slash"
export type SessionInputSuggestionLoader = (input: {
  trigger: SessionInputTrigger;
  query: string;
}) => Promise<readonly SessionInputSuggestion[]>
export type SessionInputClasses = {
  form: string;
  editorFrame: string;
  contentEditable: string;
  placeholder: string;
  footer: string;
  helperText: string;
  submitButton: string;
}

type DismissedMenuState = Pick<SessionInputMenuState, "nodeKey" | "startOffset" | "trigger">

export component SessionInput(props: {
  autoFocus?: boolean;
  classes?: Partial<SessionInputClasses>;
  loadSuggestions: SessionInputSuggestionLoader;
  onEscape?: () => void;
  onSubmit: (prompt: SessionInputPromptBlocks) => Promise<void> | void;
  onPromptChange?: (prompt: SessionInputPromptBlocks) => void;
  placeholder?: string;
  helperText?: string;
  submitLabel?: string;
}) {
  let formElement: HTMLFormElement | undefined
  let menuElement: HTMLDivElement | null = null

  const editorRef = useRef<LexicalEditor | null>(null)
  const dismissedMenuRef = useRef<DismissedMenuState | null>(null)
  const [promptBlocks, setPromptBlocks] = useState<SessionInputPromptBlocks>([])
  const [menu, setMenu] = useState<SessionInputMenuState | null>(null)
  const [suggestions, setSuggestions] = useState<SessionInputSuggestion[]>([])
  const [selectedIndex, setSelectedIndex] = useState(0)
  const [isLoadingSuggestions, setIsLoadingSuggestions] = useState(false)
  const [isSubmitting, setIsSubmitting] = useState(false)
  const suggestionRequestIdRef = useRef(0)
  const classes = props.classes ?? {}
  const placeholder = props.placeholder ?? "Add the next instruction for this session."
  const helperText =
    props.helperText ??
      "Enter sends, Shift+Enter inserts a newline, and @, $, or / open suggestions."
  const submitLabel = props.submitLabel ?? "Send"

  const canSubmit = hasPromptContent(promptBlocks) && menu === null && !isSubmitting
  const highlightedSuggestion = suggestions[selectedIndex] ?? suggestions[0] ?? null
  const sessionInputPlaceholder = <tsx>
    <div class={classes.placeholder}>
      {placeholder}
    </div>
  </tsx>

  function clearMenuState() {
    setMenu((currentMenu) => (currentMenu === null ? currentMenu : null))
    setSuggestions(
      (currentSuggestions) => currentSuggestions.length === 0 ? currentSuggestions : [],
    )
    setSelectedIndex((currentIndex) => (currentIndex === 0 ? currentIndex : 0))
    setIsLoadingSuggestions((currentIsLoading) => currentIsLoading ? false : currentIsLoading)
  }

  function focusEditor() {
    queueMicrotask(() => {
      editorRef.current?.focus()
    })
  }

  function hideMenu(options?: {
    dismissCurrentContext?: boolean;
    preserveFocus?: boolean;
  }) {
    if (options?.dismissCurrentContext && menu) {
      dismissedMenuRef.current = {
        trigger: menu.trigger,
        nodeKey: menu.nodeKey,
        startOffset: menu.startOffset,
      }
    } else {
      dismissedMenuRef.current = null
    }

    clearMenuState()

    if (options?.preserveFocus) {
      focusEditor()
    }
  }

  function isDismissedMenu(menuState: SessionInputMenuState) {
    return dismissedMenuRef.current?.trigger === menuState.trigger &&
      dismissedMenuRef.current?.nodeKey === menuState.nodeKey &&
      dismissedMenuRef.current?.startOffset === menuState.startOffset
  }

  function isSameMenuState(
    currentMenu: SessionInputMenuState | null,
    nextMenu: SessionInputMenuState | null,
  ) {
    if (currentMenu === nextMenu) {
      return true
    }

    if (!currentMenu || !nextMenu) {
      return false
    }

    return currentMenu.trigger === nextMenu.trigger && currentMenu.query === nextMenu.query &&
      currentMenu.nodeKey === nextMenu.nodeKey &&
      currentMenu.startOffset === nextMenu.startOffset &&
      currentMenu.endOffset === nextMenu.endOffset &&
      currentMenu.anchorLeft === nextMenu.anchorLeft &&
      currentMenu.anchorTop === nextMenu.anchorTop
  }

  async function submitPrompt() {
    if (!hasPromptContent(promptBlocks) || menu !== null || isSubmitting) {
      return
    }

    setIsSubmitting(true)

    try {
      await props.onSubmit(promptBlocks)
      dismissedMenuRef.current = null
      setPromptBlocks([])
      props.onPromptChange?.([])

      if (editorRef.current) {
        clearSessionInputEditor(editorRef.current)
        focusEditor()
      }
    } finally {
      setIsSubmitting(false)
    }
  }

  function acceptSuggestion(suggestion: SessionInputSuggestion | null) {
    if (!menu || !suggestion || !editorRef.current) {
      return
    }

    insertSessionInputSuggestion(editorRef.current, menu, suggestion)
    dismissedMenuRef.current = null
    clearMenuState()
    focusEditor()
  }

  function acceptHighlightedSuggestion() {
    acceptSuggestion(highlightedSuggestion)
  }

  function highlightNextSuggestion() {
    setSelectedIndex(
      (currentIndex) => suggestions.length === 0 ? 0 : (currentIndex + 1) % suggestions.length,
    )
  }

  function highlightPreviousSuggestion() {
    setSelectedIndex(
      (currentIndex) => suggestions.length === 0
        ? 0
        : (currentIndex - 1 + suggestions.length) % suggestions.length,
    )
  }

  function highlightSuggestion(index: number) {
    setSelectedIndex(index)
  }

  function updateMenu(nextMenu: SessionInputMenuState | null) {
    if (!nextMenu) {
      dismissedMenuRef.current = null
      clearMenuState()
      return
    }

    if (isDismissedMenu(nextMenu)) {
      clearMenuState()
      return
    }

    setMenu((currentMenu) => isSameMenuState(currentMenu, nextMenu) ? currentMenu : nextMenu)
  }

  function updatePromptBlocks(nextPromptBlocks: SessionInputPromptBlocks) {
    setPromptBlocks(nextPromptBlocks)
    props.onPromptChange?.(nextPromptBlocks)
  }

  function registerEditor(editor: LexicalEditor) {
    editorRef.current = editor

    if (props.autoFocus) {
      focusEditor()
    }
  }

  function dismissMenuFromEditor() {
    hideMenu(
      {
        dismissCurrentContext: true,
        preserveFocus: true,
      },
    )
  }

  function submitFromEditor() {
    void submitPrompt()
  }

  useEffect(() => {
    if (!menu) {
      return
    }

    const requestId = suggestionRequestIdRef.current + 1
    suggestionRequestIdRef.current = requestId
    setIsLoadingSuggestions(true)

    void props.loadSuggestions(
      {
        trigger: menu.trigger,
        query: menu.query,
      },
    ).then((response) => {
      if (suggestionRequestIdRef.current !== requestId) {
        return
      }

      setSuggestions([...response])
      setSelectedIndex(0)
    }).catch((error) => {
      if (suggestionRequestIdRef.current !== requestId) {
        return
      }

      console.error("Failed to load session input suggestions.", error)
      setSuggestions([])
      setSelectedIndex(0)
    }).finally(() => {
      if (suggestionRequestIdRef.current === requestId) {
        setIsLoadingSuggestions(false)
      }
    })
  }, [
    menu?.nodeKey,
    menu?.query,
    menu?.startOffset,
    menu?.trigger,
    props.loadSuggestions,
  ])

  useEffect(() => {
    setSelectedIndex((currentIndex) => {
      if (suggestions.length === 0) {
        return 0
      }

      return Math.min(currentIndex, suggestions.length - 1)
    })
  }, [suggestions])

  useListener(document, "mousedown", (event) => {
    if (!menu) {
      return
    }

    const target = event.target

    if (!(target instanceof Node)) {
      return
    }

    if (menuElement?.contains(target) || formElement?.contains(target)) {
      return
    }

    hideMenu()
  })

  useListener(window, "resize", () => {
    if (!menu) {
      return
    }

    hideMenu()
  })

  <form
    {ref formElement}
    class={classes.form}
    onSubmit={(event) => {
      event.preventDefault()
      void submitPrompt()
    }}
  >
    <LexicalComposer initialConfig={sessionInputInitialConfig}>
      <div class={classes.editorFrame}>
        <PlainTextPlugin
          ErrorBoundary={LexicalErrorBoundary}
          contentEditable={<tsx>
            <ContentEditable aria-placeholder={placeholder} class={classes.contentEditable} placeholder={sessionInputPlaceholder} />
          </tsx>}
          placeholder={sessionInputPlaceholder}
        />
        <SessionInputPlugins
          {menu}
          onAcceptMenu={acceptHighlightedSuggestion}
          onDismissMenu={dismissMenuFromEditor}
          onEditorReady={registerEditor}
          onHighlightNext={highlightNextSuggestion}
          onHighlightPrevious={highlightPreviousSuggestion}
          onMenuChange={updateMenu}
          onPromptBlocksChange={updatePromptBlocks}
          onEscape={props.onEscape}
          onSubmit={submitFromEditor}
        />
      </div>
    </LexicalComposer>
    <SessionInputMenu
      {isLoadingSuggestions}
      {menu}
      menuRef={(element) => {
        menuElement = element
      }}
      {selectedIndex}
      {suggestions}
      onAcceptSuggestion={acceptSuggestion}
      onHighlight={highlightSuggestion}
    />
    <div class={classes.footer}>
      <p class={classes.helperText}>
        {helperText}
      </p>
      <button class={classes.submitButton} disabled={!canSubmit} type="submit">
        {submitLabel}
        <SendHorizontal size={15} strokeWidth={2.2} />
      </button>
    </div>
  </form>
}
