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
  removeTriggerToken,
  SessionInputPlugins,
  sessionInputInitialConfig,
  type SessionInputMenuState,
} from "./input-lexical.tsx"
import { SessionInputMenu } from "./input-menu.tsx"

export type SessionInputSuggestion = SessionComposerSuggestionsResponse["suggestions"][number]
export type SessionInputPromptBlocks = Exclude<SessionPromptRequest["prompt"], string>
export type SessionInputTrigger = "at" | "dollar" | "slash"
export type SessionInputSuggestionLoader = (input: {
  trigger: SessionInputTrigger
  query: string
}) => Promise<readonly SessionInputSuggestion[]>
export type SessionInputClasses = {
  form: string
  editorFrame: string
  contentEditable: string
  placeholder: string
  footer: string
  helperText: string
  submitButton: string
}

export function SessionInput(props: {
  classes?: Partial<SessionInputClasses>
  loadSuggestions: SessionInputSuggestionLoader
  onSubmit: (prompt: SessionInputPromptBlocks) => Promise<void> | void
  onPromptChange?: (prompt: SessionInputPromptBlocks) => void
  placeholder?: string
  helperText?: string
  submitLabel?: string
}) {
  const formRef = useRef<HTMLFormElement | null>(null)
  const menuRef = useRef<HTMLDivElement | null>(null)
  const editorRef = useRef<LexicalEditor | null>(null)
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
  const sessionInputPlaceholder = <div class={classes.placeholder}>{placeholder}</div>

  function focusEditor() {
    queueMicrotask(() => {
      editorRef.current?.focus()
    })
  }

  function closeMenu(options: { removeToken: boolean }) {
    const activeEditor = editorRef.current
    const activeMenu = menu

    setMenu(null)
    setSuggestions([])
    setSelectedIndex(0)
    setIsLoadingSuggestions(false)

    if (options.removeToken && activeEditor && activeMenu) {
      removeTriggerToken(activeEditor, activeMenu)
    }

    focusEditor()
  }

  async function submitPrompt() {
    if (!hasPromptContent(promptBlocks) || menu !== null || isSubmitting) {
      return
    }

    setIsSubmitting(true)

    try {
      await props.onSubmit(promptBlocks)
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
    setMenu(null)
    setSuggestions([])
    setSelectedIndex(0)
    setIsLoadingSuggestions(false)
    focusEditor()
  }

  function acceptHighlightedSuggestion() {
    acceptSuggestion(highlightedSuggestion)
  }

  useEffect(() => {
    if (!menu) {
      return
    }

    const requestId = suggestionRequestIdRef.current + 1
    suggestionRequestIdRef.current = requestId
    setIsLoadingSuggestions(true)

    void props
      .loadSuggestions({
        trigger: menu.trigger,
        query: menu.query,
      })
      .then((response) => {
        if (suggestionRequestIdRef.current !== requestId) {
          return
        }

        setSuggestions([...response])
        setSelectedIndex(0)
      })
      .catch((error) => {
        if (suggestionRequestIdRef.current !== requestId) {
          return
        }

        console.error("Failed to load session input suggestions.", error)
        setSuggestions([])
        setSelectedIndex(0)
      })
      .finally(() => {
        if (suggestionRequestIdRef.current === requestId) {
          setIsLoadingSuggestions(false)
        }
      })
  }, [menu, props.loadSuggestions])

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

    if (menuRef.current?.contains(target) || formRef.current?.contains(target)) {
      return
    }

    closeMenu({ removeToken: true })
  })

  useListener(window, "resize", () => {
    if (!menu) {
      return
    }

    closeMenu({ removeToken: true })
  })

  return (
    <form
      ref={formRef}
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
            contentEditable={
              <ContentEditable
                aria-placeholder={placeholder}
                class={classes.contentEditable}
                placeholder={sessionInputPlaceholder}
              />
            }
            placeholder={sessionInputPlaceholder}
          />
          <SessionInputPlugins
            menu={menu}
            onAcceptMenu={acceptHighlightedSuggestion}
            onCancelMenu={() => {
              closeMenu({ removeToken: true })
            }}
            onEditorReady={(editor) => {
              editorRef.current = editor
            }}
            onPromptBlocksChange={(nextPromptBlocks) => {
              setPromptBlocks(nextPromptBlocks)
              props.onPromptChange?.(nextPromptBlocks)
            }}
            onSubmit={() => {
              void submitPrompt()
            }}
            onTriggerDetected={(nextMenu) => {
              setMenu(nextMenu)
            }}
          />
        </div>
      </LexicalComposer>

      <SessionInputMenu
        isLoadingSuggestions={isLoadingSuggestions}
        menu={menu}
        menuRef={menuRef}
        selectedIndex={selectedIndex}
        suggestions={suggestions}
        onAcceptHighlighted={acceptHighlightedSuggestion}
        onAcceptSuggestion={acceptSuggestion}
        onClose={() => {
          closeMenu({ removeToken: true })
        }}
        onHighlight={(index) => {
          setSelectedIndex(index)
        }}
        onHighlightNext={() => {
          setSelectedIndex((currentIndex) =>
            suggestions.length === 0 ? 0 : (currentIndex + 1) % suggestions.length,
          )
        }}
        onHighlightPrevious={() => {
          setSelectedIndex((currentIndex) =>
            suggestions.length === 0
              ? 0
              : (currentIndex - 1 + suggestions.length) % suggestions.length,
          )
        }}
        onQueryChange={(query) => {
          setMenu((currentMenu) =>
            currentMenu
              ? {
                  ...currentMenu,
                  query,
                }
              : null,
          )
        }}
      />

      <div class={classes.footer}>
        <p class={classes.helperText}>{helperText}</p>
        <button class={classes.submitButton} disabled={!canSubmit} type="submit">
          {submitLabel}
          <SendHorizontal size={15} strokeWidth={2.2} />
        </button>
      </div>
    </form>
  )
}
