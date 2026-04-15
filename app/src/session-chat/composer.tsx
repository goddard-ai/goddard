import type { SessionComposerSuggestionsResponse, SessionPromptRequest } from "@goddard-ai/sdk"
import { css, cx } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { LexicalComposer } from "@lexical/react/LexicalComposer"
import { useLexicalComposerContext } from "@lexical/react/LexicalComposerContext"
import { ContentEditable } from "@lexical/react/LexicalContentEditable"
import { LexicalErrorBoundary } from "@lexical/react/LexicalErrorBoundary"
import { HistoryPlugin } from "@lexical/react/LexicalHistoryPlugin"
import { OnChangePlugin } from "@lexical/react/LexicalOnChangePlugin"
import { PlainTextPlugin } from "@lexical/react/LexicalPlainTextPlugin"
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
  KEY_ENTER_COMMAND,
  KEY_ESCAPE_COMMAND,
} from "lexical"
import { BookOpen, Command, File, Folder, LoaderCircle, SendHorizontal } from "lucide-react"
import { useListener } from "preact-sigma"
import { useEffect, useRef, useState } from "preact/hooks"

import {
  inputMenuBodyClass,
  inputMenuButtonActiveClass,
  inputMenuButtonClass,
  inputMenuClass,
  inputMenuDetailClass,
  inputMenuEmptyClass,
  inputMenuFilterClass,
  inputMenuHeaderClass,
  inputMenuIconClass,
  inputMenuLabelClass,
  inputMenuListClass,
} from "~/session-input/menu-styles.ts"
import {
  $createComposerChipNode,
  ComposerChipNode,
  type ComposerChipData,
} from "./composer-chip-node.tsx"
import { hasPromptContent, serializeComposerEditorState } from "./composer-content.ts"

export type ComposerSuggestion = SessionComposerSuggestionsResponse["suggestions"][number]
export type ComposerPromptBlocks = Exclude<SessionPromptRequest["prompt"], string>
export type ComposerTrigger = "at" | "dollar" | "slash"
export type ComposerSuggestionLoader = (input: {
  trigger: ComposerTrigger
  query: string
}) => Promise<readonly ComposerSuggestion[]>

type ComposerMenuState = {
  trigger: ComposerTrigger
  query: string
  nodeKey: NodeKey
  startOffset: number
  endOffset: number
  anchorLeft: number
  anchorTop: number
}

const composerFormClass = css({
  display: "grid",
  gap: "14px",
  padding: "16px 18px 18px",
  borderTop: "1px solid",
  borderColor: "border",
  background: `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.panel")} 100%)`,
})

const editorFrameClass = css({
  position: "relative",
  borderRadius: "18px",
  border: "1px solid",
  borderColor: "border",
  backgroundColor: "background",
  transition:
    "border-color 160ms cubic-bezier(0.23, 1, 0.32, 1), box-shadow 160ms cubic-bezier(0.23, 1, 0.32, 1)",
  _focusWithin: {
    borderColor: "accentStrong",
    boxShadow: `0 0 0 3px color-mix(in srgb, ${token.var("colors.accent")} 16%, transparent)`,
  },
})

const contentEditableClass = css({
  width: "100%",
  minHeight: "96px",
  padding: "14px 16px",
  color: "text",
  fontSize: "0.94rem",
  lineHeight: "1.6",
  outline: "none",
  whiteSpace: "pre-wrap",
  wordBreak: "break-word",
})

const placeholderClass = css({
  position: "absolute",
  inset: "14px 16px auto",
  color: "muted",
  fontSize: "0.94rem",
  lineHeight: "1.6",
  pointerEvents: "none",
})

const footerClass = css({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: "12px",
})

const helperTextClass = css({
  color: "muted",
  fontSize: "0.83rem",
  lineHeight: "1.6",
})

const submitButtonClass = css({
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  gap: "8px",
  minWidth: "112px",
  height: "40px",
  paddingInline: "14px",
  borderRadius: "14px",
  border: "1px solid",
  borderColor: "accent",
  background: `linear-gradient(180deg, ${token.var("colors.surface")} 0%, ${token.var("colors.background")} 100%)`,
  color: "text",
  fontSize: "0.88rem",
  fontWeight: "680",
  cursor: "pointer",
  _disabled: {
    cursor: "not-allowed",
    opacity: "0.52",
  },
})

const composerInitialConfig = {
  namespace: "session-chat-composer",
  nodes: [ComposerChipNode],
  onError(error: Error) {
    throw error
  },
}

function getMenuHeading(trigger: ComposerTrigger) {
  if (trigger === "at") {
    return "@ Files And Folders"
  }

  if (trigger === "dollar") {
    return "$ Skills"
  }

  return "/ Slash Commands"
}

function getSuggestionLabel(suggestion: ComposerSuggestion) {
  return suggestion.type === "slash_command" ? `/${suggestion.name}` : suggestion.label
}

function getSuggestionDetail(suggestion: ComposerSuggestion) {
  if (suggestion.type === "slash_command") {
    return suggestion.inputHint ?? suggestion.description
  }

  return suggestion.detail
}

function suggestionKey(suggestion: ComposerSuggestion) {
  if (suggestion.type === "slash_command") {
    return `slash:${suggestion.name}`
  }

  return `${suggestion.type}:${suggestion.path}`
}

function suggestionToChip(suggestion: ComposerSuggestion): ComposerChipData {
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

function SuggestionIcon(props: { suggestion: ComposerSuggestion }) {
  if (props.suggestion.type === "folder") {
    return <Folder size={14} strokeWidth={2.2} />
  }

  if (props.suggestion.type === "file") {
    return <File size={14} strokeWidth={2.2} />
  }

  if (props.suggestion.type === "skill") {
    return <BookOpen size={14} strokeWidth={2.2} />
  }

  return <Command size={14} strokeWidth={2.2} />
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

function detectComposerMenuState() {
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

  return {
    trigger: match[1] === "@" ? "at" : match[1] === "$" ? "dollar" : "slash",
    query: match[2] ?? "",
    nodeKey: anchorNode.getKey(),
    startOffset: anchorOffset - triggerToken.length,
    endOffset: anchorOffset,
  } satisfies Omit<ComposerMenuState, "anchorLeft" | "anchorTop">
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

function clearComposerEditor(editor: LexicalEditor) {
  editor.update(
    () => {
      const root = $getRoot()
      root.clear()
      root.append($createParagraphNode())
    },
    { discrete: true },
  )
}

function removeTriggerToken(editor: LexicalEditor, menu: ComposerMenuState) {
  editor.update(
    () => {
      const targetNode = $getNodeByKey(menu.nodeKey)

      if (!$isTextNode(targetNode)) {
        return
      }

      targetNode.spliceText(menu.startOffset, menu.endOffset - menu.startOffset, "", false)
      targetNode.select(menu.startOffset, menu.startOffset)
    },
    { discrete: true },
  )
}

function replaceTriggerTokenWithChip(
  editor: LexicalEditor,
  menu: ComposerMenuState,
  chip: ComposerChipData,
) {
  editor.update(
    () => {
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
    },
    { discrete: true },
  )
}

function ComposerPlugins(props: {
  menu: ComposerMenuState | null
  onEditorReady: (editor: LexicalEditor) => void
  onPromptBlocksChange: (blocks: ComposerPromptBlocks) => void
  onTriggerDetected: (menu: ComposerMenuState) => void
  onSubmit: () => void
  onCancelMenu: () => void
  onAcceptMenu: () => void
}) {
  const [editor] = useLexicalComposerContext()

  useEffect(() => {
    props.onEditorReady(editor)
  }, [editor, props])

  useEffect(() => {
    return editor.registerUpdateListener(({ editorState }) => {
      if (props.menu) {
        return
      }

      const detectedMenu = editorState.read(() => detectComposerMenuState())

      if (!detectedMenu) {
        return
      }

      props.onTriggerDetected({
        trigger: detectedMenu.trigger,
        query: detectedMenu.query,
        nodeKey: detectedMenu.nodeKey,
        startOffset: detectedMenu.startOffset,
        endOffset: detectedMenu.endOffset,
        ...getCaretAnchorPosition(editor),
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
    const unregisterEscape = editor.registerCommand(
      KEY_ESCAPE_COMMAND,
      (event) => {
        if (!props.menu) {
          return false
        }

        event?.preventDefault()
        props.onCancelMenu()
        return true
      },
      COMMAND_PRIORITY_HIGH,
    )

    return () => {
      unregisterEnter()
      unregisterEscape()
    }
  }, [editor, props])

  return (
    <>
      <HistoryPlugin />
      <OnChangePlugin
        ignoreSelectionChange={true}
        onChange={(editorState) => {
          props.onPromptBlocksChange(serializeComposerEditorState(editorState))
        }}
      />
    </>
  )
}

export function Composer(props: {
  loadSuggestions: ComposerSuggestionLoader
  onSubmit: (prompt: ComposerPromptBlocks) => Promise<void> | void
  onPromptChange?: (prompt: ComposerPromptBlocks) => void
  placeholder?: string
  helperText?: string
  submitLabel?: string
}) {
  const formRef = useRef<HTMLFormElement | null>(null)
  const menuRef = useRef<HTMLDivElement | null>(null)
  const filterInputRef = useRef<HTMLInputElement | null>(null)
  const editorRef = useRef<LexicalEditor | null>(null)
  const [promptBlocks, setPromptBlocks] = useState<ComposerPromptBlocks>([])
  const [menu, setMenu] = useState<ComposerMenuState | null>(null)
  const [suggestions, setSuggestions] = useState<ComposerSuggestion[]>([])
  const [selectedIndex, setSelectedIndex] = useState(0)
  const [isLoadingSuggestions, setIsLoadingSuggestions] = useState(false)
  const [isSubmitting, setIsSubmitting] = useState(false)
  const suggestionRequestIdRef = useRef(0)
  const placeholder = props.placeholder ?? "Add the next instruction for this session."
  const helperText =
    props.helperText ??
    "Enter sends, Shift+Enter inserts a newline, and @, $, or / open suggestions."
  const submitLabel = props.submitLabel ?? "Send"

  const canSubmit = hasPromptContent(promptBlocks) && menu === null && !isSubmitting
  const highlightedSuggestion = suggestions[selectedIndex] ?? suggestions[0] ?? null
  const composerPlaceholder = <div class={placeholderClass}>{placeholder}</div>

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
        clearComposerEditor(editorRef.current)
        focusEditor()
      }
    } finally {
      setIsSubmitting(false)
    }
  }

  function acceptSuggestion(suggestion: ComposerSuggestion | null) {
    if (!menu || !suggestion || !editorRef.current) {
      return
    }

    replaceTriggerTokenWithChip(editorRef.current, menu, suggestionToChip(suggestion))
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

        console.error("Failed to load composer suggestions.", error)
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
    if (!menu) {
      return
    }

    const input = filterInputRef.current

    if (!input) {
      return
    }

    input.focus()
    input.setSelectionRange(input.value.length, input.value.length)
  }, [menu])

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
      class={composerFormClass}
      onSubmit={(event) => {
        event.preventDefault()
        void submitPrompt()
      }}
    >
      <LexicalComposer initialConfig={composerInitialConfig}>
        <div class={editorFrameClass}>
          <PlainTextPlugin
            ErrorBoundary={LexicalErrorBoundary}
            contentEditable={
              <ContentEditable
                aria-placeholder={placeholder}
                class={contentEditableClass}
                placeholder={composerPlaceholder}
              />
            }
            placeholder={composerPlaceholder}
          />
          <ComposerPlugins
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

      {menu ? (
        <div
          ref={menuRef}
          class={inputMenuClass}
          style={{
            left: `${menu.anchorLeft}px`,
            top: `${menu.anchorTop}px`,
          }}
        >
          <div class={inputMenuHeaderClass}>{getMenuHeading(menu.trigger)}</div>
          <input
            ref={filterInputRef}
            class={inputMenuFilterClass}
            placeholder="Type to filter"
            value={menu.query}
            onInput={(event) => {
              setMenu((currentMenu) =>
                currentMenu
                  ? {
                      ...currentMenu,
                      query: event.currentTarget.value,
                    }
                  : null,
              )
            }}
            onKeyDown={(event) => {
              if (event.key === "ArrowDown") {
                event.preventDefault()
                setSelectedIndex((currentIndex) =>
                  suggestions.length === 0 ? 0 : (currentIndex + 1) % suggestions.length,
                )
                return
              }

              if (event.key === "ArrowUp") {
                event.preventDefault()
                setSelectedIndex((currentIndex) =>
                  suggestions.length === 0
                    ? 0
                    : (currentIndex - 1 + suggestions.length) % suggestions.length,
                )
                return
              }

              if (event.key === "Enter") {
                event.preventDefault()
                acceptHighlightedSuggestion()
                return
              }

              if (event.key === "Escape") {
                event.preventDefault()
                closeMenu({ removeToken: true })
              }
            }}
          />
          <div class={inputMenuListClass}>
            {isLoadingSuggestions ? (
              <div class={inputMenuEmptyClass}>
                <span
                  class={css({
                    display: "inline-flex",
                    alignItems: "center",
                    gap: "8px",
                  })}
                >
                  <LoaderCircle class={css({ animation: "spin 1s linear infinite" })} size={14} />
                  Loading suggestions...
                </span>
              </div>
            ) : suggestions.length === 0 ? (
              <div class={inputMenuEmptyClass}>No matches for this trigger yet.</div>
            ) : (
              suggestions.map((suggestion, index) => (
                <button
                  key={suggestionKey(suggestion)}
                  class={cx(
                    inputMenuButtonClass,
                    index === selectedIndex ? inputMenuButtonActiveClass : undefined,
                  )}
                  type="button"
                  onMouseDown={(event) => {
                    event.preventDefault()
                  }}
                  onMouseEnter={() => {
                    setSelectedIndex(index)
                  }}
                  onClick={() => {
                    acceptSuggestion(suggestion)
                  }}
                >
                  <span class={inputMenuIconClass} aria-hidden="true">
                    <SuggestionIcon suggestion={suggestion} />
                  </span>
                  <span class={inputMenuBodyClass}>
                    <span class={inputMenuLabelClass}>{getSuggestionLabel(suggestion)}</span>
                    <span class={inputMenuDetailClass}>{getSuggestionDetail(suggestion)}</span>
                  </span>
                </button>
              ))
            )}
          </div>
        </div>
      ) : null}

      <div class={footerClass}>
        <p class={helperTextClass}>{helperText}</p>
        <button class={submitButtonClass} disabled={!canSubmit} type="submit">
          {submitLabel}
          <SendHorizontal size={15} strokeWidth={2.2} />
        </button>
      </div>
    </form>
  )
}
