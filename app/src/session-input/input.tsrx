import type { SessionComposerSuggestionsResponse, SessionPromptRequest } from "@goddard-ai/sdk"
import { useListNavigation } from "@goddard-ai/ui-primitives"
import { LexicalComposer } from "@lexical/react/LexicalComposer"
import { ContentEditable } from "@lexical/react/LexicalContentEditable"
import { LexicalErrorBoundary } from "@lexical/react/LexicalErrorBoundary"
import { RichTextPlugin } from "@lexical/react/LexicalRichTextPlugin"
import type { LexicalEditor } from "lexical"
import { SendHorizontal } from "lucide-react"
import { useListener } from "preact-sigma"
import { useEffect, useRef, useState } from "preact/hooks"

import { hasPromptContent } from "~/session-chat/composer-content.ts"
import { GoodKeyboardShortcut } from "~/lib/good-keyboard-shortcut.tsrx"
import { GoodTooltip } from "~/lib/good-tooltip.tsrx"
import {
  clearSessionInputEditor,
  insertSessionInputSuggestion,
  setSessionInputEditorPrompt,
} from "./input-editor-content.ts"
import type { SessionInputMenuState } from "./input-menu-detection.ts"
import { SessionInputPlugins, sessionInputInitialConfig } from "./input-lexical.tsrx"
import { SessionInputMenu } from "./input-menu.tsrx"
import { registerSessionInputPromptFocus } from "./prompt-focus.ts"

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

export function SessionInput(props: {
  activationFocus?: boolean;
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
  contextUsage?: SessionInputContextUsage | null;
  submitLabel?: string;
}) @{
  const formElementRef = useRef<HTMLFormElement>(null)
  const menuElementRef = useRef<HTMLDivElement | null>(null)
  const editorRef = useRef<LexicalEditor | null>(null)
  const unregisterPromptFocusRef = useRef<(() => void) | null>(null)
  const dismissedMenuRef = useRef<DismissedMenuState | null>(null)
  const [promptBlocks, setPromptBlocks] = useState<SessionInputPromptBlocks>(
    () => props.initialPrompt ?? [],
  )
  const [menu, setMenu] = useState<SessionInputMenuState | null>(null)
  const [suggestions, setSuggestions] = useState<SessionInputSuggestion[]>([])
  const [isLoadingSuggestions, setIsLoadingSuggestions] = useState(false)
  const [isSubmitting, setIsSubmitting] = useState(false)
  const suggestionRequestIdRef = useRef(0)
  const classes = props.classes ?? {}
  const placeholder = props.placeholder ?? "Add the next instruction for this session."
  const helperText =
    props.helperText === undefined
      ? <>
          <GoodKeyboardShortcut expression="Enter" />
          {" sends at the end, "}
          <GoodKeyboardShortcut expression="Mod+Enter" />
          {" sends anywhere, "}
          <GoodKeyboardShortcut expression="Shift+Enter" />
          {" inserts a newline, and "}
          <span>@, $, or /</span>
          {" open suggestions."}
        </>
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
  const navigation = useListNavigation({
    activeAttribute: "aria-selected",
    count: () => suggestions.length,
    onActivate(index) {
      acceptSuggestion(suggestions[index] ?? null)
    },
  })
  const activeSuggestionIndex = navigation.activeIndex()
  const highlightedSuggestion = suggestions.at(activeSuggestionIndex) ?? suggestions[0] ?? null
  function clearMenuState() {
    setMenu((currentMenu) => (currentMenu === null ? currentMenu : null))
    setSuggestions(
      (currentSuggestions) => currentSuggestions.length === 0 ? currentSuggestions : [],
    )
    navigation.resetActiveIndex()
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

  function isSameMenuContext(
    currentMenu: SessionInputMenuState | null,
    nextMenu: SessionInputMenuState | null,
  ) {
    if (currentMenu === nextMenu) {
      return true
    }

    if (!currentMenu || !nextMenu) {
      return false
    }

    return currentMenu.trigger === nextMenu.trigger && currentMenu.nodeKey === nextMenu.nodeKey &&
      currentMenu.startOffset === nextMenu.startOffset
  }

  function isSameMenuState(
    currentMenu: SessionInputMenuState | null,
    nextMenu: SessionInputMenuState | null,
  ) {
    if (!isSameMenuContext(currentMenu, nextMenu)) {
      return false
    }

    return currentMenu?.query === nextMenu?.query &&
      currentMenu?.endOffset === nextMenu?.endOffset &&
      currentMenu?.anchorLeft === nextMenu?.anchorLeft &&
      currentMenu?.anchorTop === nextMenu?.anchorTop
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
    navigation.moveActiveIndex(1)
  }

  function highlightPreviousSuggestion() {
    navigation.moveActiveIndex(-1)
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

    setMenu((currentMenu) => {
      if (!currentMenu) {
        return nextMenu
      }

      if (!isSameMenuContext(currentMenu, nextMenu)) {
        return nextMenu
      }

      return isSameMenuState(currentMenu, nextMenu)
        ? currentMenu
        : {
            ...nextMenu,
            anchorLeft: currentMenu.anchorLeft,
            anchorTop: currentMenu.anchorTop,
          }
    })
  }

  function updatePromptBlocks(nextPromptBlocks: SessionInputPromptBlocks) {
    setPromptBlocks(nextPromptBlocks)
    props.onPromptChange?.(nextPromptBlocks)
  }

  function registerEditor(editor: LexicalEditor) {
    if (editorRef.current !== editor) {
      unregisterPromptFocusRef.current?.()
      unregisterPromptFocusRef.current = registerSessionInputPromptFocus(editor)
    }

    editorRef.current = editor
    setSessionInputEditorPrompt(editor, promptBlocks)

    if (props.autoFocus) {
      focusEditor()
    }
  }

  useEffect(
    () => () => {
      unregisterPromptFocusRef.current?.()
      unregisterPromptFocusRef.current = null
    },
    [],
  )

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
      navigation.resetActiveIndex()
    }).catch((error) => {
      if (suggestionRequestIdRef.current !== requestId) {
        return
      }

      console.error("Failed to load session input suggestions.", error)
      setSuggestions([])
      navigation.resetActiveIndex()
    }).finally(() => {
      if (suggestionRequestIdRef.current === requestId) {
        setIsLoadingSuggestions(false)
      }
    })
  }, [
    disabled,
    menu?.nodeKey,
    menu?.query,
    menu?.startOffset,
    menu?.trigger,
    navigation,
    props.loadSuggestions,
  ])

  useEffect(() => {
    navigation.setActiveIndex(navigation.activeIndex())
  }, [navigation, suggestions])

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
    ref={formElementRef}
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
          contentEditable={<ContentEditable
            aria-disabled={disabled}
            aria-label={placeholder}
            class={classes.contentEditable}
            contentEditable={!disabled}
            data-workbench-activation-focus={props.activationFocus ? "true" : undefined}
          />}
          placeholder={<div class={classes.placeholder}>{placeholder}</div>}
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

        @if (props.footerInsideFrame) {
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
      itemRef={navigation.itemRef}
      {suggestions}
      onAcceptSuggestion={acceptSuggestion}
      onDismiss={() => {
        hideMenu()
      }}
    />

    @if (!props.footerInsideFrame) {
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

function SessionInputFooter(props: {
  canSubmit: boolean;
  classes: Partial<SessionInputClasses>;
  contextUsage: SessionInputContextUsage | null;
  contextUsagePercent: number | null;
  footerControls?: preact.ComponentChildren;
  helperText: preact.ComponentChildren;
  iconOnlySubmit?: boolean;
  submitLabel: string;
}) @{
  <div class={props.classes.footer}>
    @if (props.footerControls) {
      <div class={props.classes.footerControls}>{props.footerControls}</div>
    }
    @if (props.helperText) {
      <p class={props.classes.helperText}>{props.helperText}</p>
    }
    @if (props.contextUsage) {
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
      @if (!props.iconOnlySubmit) {
        <>
          {props.submitLabel}
        </>
      }

      <SendHorizontal size={15} strokeWidth={2.2} />
    </button>
  </div>
}
