import { Portal } from "@ark-ui/react/portal"
import { css, cx } from "@goddard-ai/styled-system/css"
import { BookOpen, Command, File, Folder, LoaderCircle } from "lucide-react"
import { useEffect, useRef } from "preact/hooks"

import type { SessionInputMenuState } from "./input-lexical.tsx"
import type { SessionInputSuggestion, SessionInputTrigger } from "./input.tsx"
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
} from "./menu-styles.ts"

function getMenuHeading(trigger: SessionInputTrigger) {
  if (trigger === "at") {
    return "@ Files And Folders"
  }

  if (trigger === "dollar") {
    return "$ Skills"
  }

  return "/ Slash Commands"
}

function getSuggestionLabel(suggestion: SessionInputSuggestion) {
  return suggestion.type === "slash_command" ? `/${suggestion.name}` : suggestion.label
}

function getSuggestionDetail(suggestion: SessionInputSuggestion) {
  if (suggestion.type === "slash_command") {
    return suggestion.inputHint ?? suggestion.description
  }

  return suggestion.detail
}

function suggestionKey(suggestion: SessionInputSuggestion) {
  if (suggestion.type === "slash_command") {
    return `slash:${suggestion.name}`
  }

  return `${suggestion.type}:${suggestion.path}`
}

function SuggestionIcon(props: { suggestion: SessionInputSuggestion }) {
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

export function SessionInputMenu(props: {
  isLoadingSuggestions: boolean
  menu: SessionInputMenuState | null
  menuRef: preact.RefObject<HTMLDivElement | null>
  onAcceptHighlighted: () => void
  onAcceptSuggestion: (suggestion: SessionInputSuggestion) => void
  onClose: () => void
  onHighlight: (index: number) => void
  onHighlightNext: () => void
  onHighlightPrevious: () => void
  onQueryChange: (query: string) => void
  selectedIndex: number
  suggestions: readonly SessionInputSuggestion[]
}) {
  const filterInputRef = useRef<HTMLInputElement | null>(null)

  useEffect(() => {
    if (!props.menu) {
      return
    }

    const input = filterInputRef.current

    if (!input) {
      return
    }

    input.focus()
    input.setSelectionRange(input.value.length, input.value.length)
  }, [props.menu])

  if (!props.menu) {
    return null
  }

  return (
    <Portal>
      <div
        ref={props.menuRef}
        class={inputMenuClass}
        style={{
          left: `${props.menu.anchorLeft}px`,
          top: `${props.menu.anchorTop}px`,
        }}
      >
        <div class={inputMenuHeaderClass}>{getMenuHeading(props.menu.trigger)}</div>
        <input
          ref={filterInputRef}
          class={inputMenuFilterClass}
          placeholder="Type to filter"
          value={props.menu.query}
          onInput={(event) => {
            props.onQueryChange(event.currentTarget.value)
          }}
          onKeyDown={(event) => {
            if (event.key === "ArrowDown") {
              event.preventDefault()
              props.onHighlightNext()
              return
            }

            if (event.key === "ArrowUp") {
              event.preventDefault()
              props.onHighlightPrevious()
              return
            }

            if (event.key === "Enter") {
              event.preventDefault()
              props.onAcceptHighlighted()
              return
            }

            if (event.key === "Escape") {
              event.preventDefault()
              props.onClose()
            }
          }}
        />
        <div class={inputMenuListClass}>
          {props.isLoadingSuggestions ? (
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
          ) : props.suggestions.length === 0 ? (
            <div class={inputMenuEmptyClass}>No matches for this trigger yet.</div>
          ) : (
            props.suggestions.map((suggestion, index) => (
              <button
                key={suggestionKey(suggestion)}
                class={cx(
                  inputMenuButtonClass,
                  index === props.selectedIndex ? inputMenuButtonActiveClass : undefined,
                )}
                type="button"
                onMouseDown={(event) => {
                  event.preventDefault()
                }}
                onMouseEnter={() => {
                  props.onHighlight(index)
                }}
                onClick={() => {
                  props.onAcceptSuggestion(suggestion)
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
    </Portal>
  )
}
