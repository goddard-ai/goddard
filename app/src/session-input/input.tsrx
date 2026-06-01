import type { SessionComposerSuggestionsResponse, SessionPromptRequest } from "@goddard-ai/sdk"
import { LexicalComposer } from "@lexical/react/LexicalComposer"
import { ContentEditable } from "@lexical/react/LexicalContentEditable"
import { LexicalErrorBoundary } from "@lexical/react/LexicalErrorBoundary"
import { RichTextPlugin } from "@lexical/react/LexicalRichTextPlugin"
import type { LexicalEditor } from "lexical"
import { SendHorizontal } from "lucide-react"
import { useListener } from "preact-sigma"
import { useEffect, useRef, useState } from "preact/hooks"

import { hasPromptContent } from "~/session-chat/composer-content.ts"
import GoodKeyboardShortcut from "~/lib/good-keyboard-shortcut.tsrx"
import { GoodTooltip } from "~/lib/good-tooltip.tsrx"
import {
  clearSessionInputEditor,
  insertSessionInputSuggestion,
  setSessionInputEditorPrompt,
} from "./input-editor-content.ts"
import { getSessionInputPromptHistoryIndexes, getSessionInputPromptText } from "./input-history.ts"
import type { SessionInputMenuState } from "./input-menu-detection.ts"
import { SessionInputPlugins, sessionInputInitialConfig } from "./input-lexical.tsrx"
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
  footerControls?: string;
  helperText: string;
  contextUsage?: string;
  submitButton: string;
}
export type SessionInputContextUsage = {
  size: number;
  used: number;
}

type DismissedMenuState = Pick<SessionInputMenuState, "nodeKey" | "startOffset" | "trigger">

function clonePromptBlocks(blocks: SessionInputPromptBlocks): SessionInputPromptBlocks {
  return blocks.map((block) => ({ ...block }))
}

function arePromptBlocksEqual(left: SessionInputPromptBlocks, right: SessionInputPromptBlocks) {
  return JSON.stringify(left) === JSON.stringify(right)
}

export component SessionInput(props: {
  autoFocus?: boolean;
  classes?: Partial<SessionInputClasses>;
  disabled?: boolean;
  loadSuggestions: SessionInputSuggestionLoader;
  onEscape?: () => void;
  onSubmit: (prompt: SessionInputPromptBlocks) => Promise<void> | void;
  onPromptChange?: (prompt: SessionInputPromptBlocks) => void;
  placeholder?: string;
  helperText?: preact.ComponentChildren;
  footerControls?: preact.ComponentChildren;
  footerInsideFrame?: boolean;
  iconOnlySubmit?: boolean;
  initialPrompt?: SessionInputPromptBlocks;
  promptHistory?: readonly SessionInputPromptBlocks[];
  contextUsage?: SessionInputContextUsage | null;
  submitLabel?: string;
}) {
  const formElementRef = useRef<HTMLFormElement>(null)
  const menuElementRef = useRef<HTMLDivElement | null>(null)
  const editorRef = useRef<LexicalEditor | null>(null)
  const dismissedMenuRef = useRef<DismissedMenuState | null>(null)
  const historyDraftRef = useRef<SessionInputPromptBlocks | null>(null)
  const historyFilteredIndexesRef = useRef<number[]>([])
  const historyIndexRef = useRef<number | null>(null)
  const [promptBlocks, setPromptBlocks] = useState<SessionInputPromptBlocks>(
    () => props.initialPrompt ?? [],
  )
  const [menu, setMenu] = useState<SessionInputMenuState | null>(null)
  const [suggestions, setSuggestions] = useState<SessionInputSuggestion[]>([])
  const [selectedIndex, setSelectedIndex] = useState(0)
  const [isLoadingSuggestions, setIsLoadingSuggestions] = useState(false)
  const [isSubmitting, setIsSubmitting] = useState(false)
  const suggestionRequestIdRef = useRef(0)
  const classes = props.classes ?? {}
  const placeholder = props.placeholder ?? "Add the next instruction for this session."
  const helperText =
    props.helperText === undefined
      ? <tsrx>
          <GoodKeyboardShortcut expression="Enter" />
          " sends at the end, "
          <GoodKeyboardShortcut expression="Mod+Enter" />
          " sends anywhere, "
          <GoodKeyboardShortcut expression="Shift+Enter" />
          " inserts a newline, and "
          <span>"@, $, or /"</span>
          " open suggestions."
        </tsrx>
      : props.helperText
  const submitLabel = props.submitLabel ?? "Send"
  const contextUsage =
    props.contextUsage && props.contextUsage.size > 0
      ? {
          size: props.contextUsage.size,
          used: Math.max(0, props.contextUsage.used),
        }
      : null
  const contextUsagePercent =
    contextUsage ? Math.min(100, Math.round((contextUsage.used / contextUsage.size) * 100)) : null

  const disabled = props.disabled ?? false
  const canSubmit = !disabled && hasPromptContent(promptBlocks) && menu === null && !isSubmitting
  const highlightedSuggestion = suggestions[selectedIndex] ?? suggestions[0] ?? null
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
    if (disabled || !hasPromptContent(promptBlocks) || menu !== null || isSubmitting) {
      return
    }

    setIsSubmitting(true)

    try {
      await props.onSubmit(promptBlocks)
    } catch {
      return
    } finally {
      setIsSubmitting(false)
    }

    dismissedMenuRef.current = null
    historyDraftRef.current = null
    historyFilteredIndexesRef.current = []
    historyIndexRef.current = null
    setPromptBlocks([])
    props.onPromptChange?.([])

    if (editorRef.current) {
      clearSessionInputEditor(editorRef.current)
      focusEditor()
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
    const historyIndex = historyIndexRef.current

    if (historyIndex !== null) {
      const historyPrompt = props.promptHistory?.[historyIndex]

      if (!historyPrompt || !arePromptBlocksEqual(nextPromptBlocks, historyPrompt)) {
        historyIndexRef.current = null
        historyDraftRef.current = null
        historyFilteredIndexesRef.current = []
      }
    }

    setPromptBlocks(nextPromptBlocks)
    props.onPromptChange?.(nextPromptBlocks)
  }

  function setPromptFromHistory(nextPromptBlocks: SessionInputPromptBlocks) {
    const prompt = clonePromptBlocks(nextPromptBlocks)

    setPromptBlocks(prompt)
    props.onPromptChange?.(prompt)

    if (editorRef.current) {
      setSessionInputEditorPrompt(editorRef.current, prompt)
      focusEditor()
    }
  }

  function navigatePromptHistory(direction: "next" | "previous") {
    const promptHistory = props.promptHistory ?? []

    if (promptHistory.length === 0) {
      return false
    }

    if (historyIndexRef.current === null) {
      if (direction === "next") {
        return false
      }

      const filter = getSessionInputPromptText(promptBlocks).trim().toLowerCase()
      const historyIndexes = getSessionInputPromptHistoryIndexes(promptHistory, filter)

      if (historyIndexes.length === 0) {
        return false
      }

      historyDraftRef.current = clonePromptBlocks(promptBlocks)
      historyFilteredIndexesRef.current = historyIndexes
      historyIndexRef.current = historyIndexes[historyIndexes.length - 1]
      setPromptFromHistory(promptHistory[historyIndexRef.current])
      return true
    }

    let historyIndexes = historyFilteredIndexesRef.current

    if (historyIndexes.length === 0) {
      historyIndexes = promptHistory.map((_, index) => index)
    }

    const currentFilteredIndex = historyIndexes.indexOf(historyIndexRef.current)

    if (direction === "previous") {
      const previousFilteredIndex = Math.max(0, currentFilteredIndex - 1)
      const previousHistoryIndex = historyIndexes[previousFilteredIndex]

      if (previousHistoryIndex === undefined) {
        return false
      }

      historyIndexRef.current = previousHistoryIndex
      setPromptFromHistory(promptHistory[historyIndexRef.current])
      return true
    }

    if (currentFilteredIndex >= historyIndexes.length - 1) {
      const draft = historyDraftRef.current ?? []

      historyIndexRef.current = null
      historyDraftRef.current = null
      historyFilteredIndexesRef.current = []
      setPromptFromHistory(draft)
      return true
    }

    const nextFilteredIndex = currentFilteredIndex + 1
    const nextHistoryIndex = historyIndexes[nextFilteredIndex]

    if (nextHistoryIndex === undefined) {
      return false
    }

    historyIndexRef.current = nextHistoryIndex
    setPromptFromHistory(promptHistory[historyIndexRef.current])
    return true
  }

  function registerEditor(editor: LexicalEditor) {
    editorRef.current = editor
    setSessionInputEditorPrompt(editor, promptBlocks)

    if (props.autoFocus) {
      focusEditor()
    }
  }

  function dismissMenuFromEditor() {
    hideMenu({
      dismissCurrentContext: true,
      preserveFocus: true,
    })
  }

  function submitFromEditor() {
    if (disabled) {
      return
    }

    void submitPrompt()
  }

  useEffect(() => {
    if (!menu) {
      return
    }

    if (disabled) {
      clearMenuState()
      return
    }

    const requestId = suggestionRequestIdRef.current + 1
    suggestionRequestIdRef.current = requestId
    setIsLoadingSuggestions(true)

    void props.loadSuggestions({
      trigger: menu.trigger,
      query: menu.query,
    }).then((response) => {
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

    if (menuElementRef.current?.contains(target) || formElementRef.current?.contains(target)) {
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
    {ref formElementRef}
    class={classes.form}
    onSubmit={(event) => {
      event.preventDefault()
      void submitPrompt()
    }}
  >
    <LexicalComposer initialConfig={sessionInputInitialConfig}>
      <div class={classes.editorFrame}>
        <RichTextPlugin
          ErrorBoundary={LexicalErrorBoundary}
          contentEditable={<tsrx>
            <ContentEditable
              aria-disabled={disabled}
              aria-label={placeholder}
              class={classes.contentEditable}
              contentEditable={!disabled}
            />
          </tsrx>}
          placeholder={<tsrx>
            <div class={classes.placeholder}>
              {placeholder}
            </div>
          </tsrx>}
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
          onHistoryNext={() => navigatePromptHistory("next")}
          onHistoryPrevious={() => navigatePromptHistory("previous")}
          onSubmit={submitFromEditor}
        />

        if (props.footerInsideFrame) {
          <SessionInputFooter
            {canSubmit}
            {classes}
            {contextUsage}
            {contextUsagePercent}
            footerControls={props.footerControls}
            {helperText}
            iconOnlySubmit={props.iconOnlySubmit}
            {submitLabel}
          />
        }
      </div>
    </LexicalComposer>
    <SessionInputMenu
      {isLoadingSuggestions}
      {menu}
      menuRef={(element) => {
        menuElementRef.current = element
      }}
      {selectedIndex}
      {suggestions}
      onAcceptSuggestion={acceptSuggestion}
      onHighlight={highlightSuggestion}
    />

    if (!props.footerInsideFrame) {
      <SessionInputFooter
        {canSubmit}
        {classes}
        {contextUsage}
        {contextUsagePercent}
        footerControls={props.footerControls}
        {helperText}
        iconOnlySubmit={props.iconOnlySubmit}
        {submitLabel}
      />
    }
  </form>
}

component SessionInputFooter(props: {
  canSubmit: boolean;
  classes: Partial<SessionInputClasses>;
  contextUsage: SessionInputContextUsage | null;
  contextUsagePercent: number | null;
  footerControls?: preact.ComponentChildren;
  helperText: preact.ComponentChildren;
  iconOnlySubmit?: boolean;
  submitLabel: string;
}) {
  <div class={props.classes.footer}>
    if (props.footerControls) {
      <div class={props.classes.footerControls}>
        {props.footerControls}
      </div>
    }
    if (props.helperText) {
      <p class={props.classes.helperText}>
        {props.helperText}
      </p>
    }
    if (props.contextUsage) {
      <GoodTooltip
        ariaLabel={`Context usage ${props.contextUsagePercent}%`}
        content={`${props.contextUsagePercent}%`}
      >
        <div class={props.classes.contextUsage}>
          <svg aria-hidden="true" viewBox="0 0 36 36">
            <circle cx="18" cy="18" r="15.5" />
            <circle
              cx="18"
              cy="18"
              r="15.5"
              pathLength="100"
              strokeDasharray={`${props.contextUsagePercent} 100`}
            />
          </svg>
        </div>
      </GoodTooltip>
    }

    <button
      aria-label={props.submitLabel}
      class={props.classes.submitButton}
      disabled={!props.canSubmit}
      type="submit"
    >
      if (!props.iconOnlySubmit) {
        {props.submitLabel}
      }

      <SendHorizontal size={15} strokeWidth={2.2} />
    </button>
  </div>
}
