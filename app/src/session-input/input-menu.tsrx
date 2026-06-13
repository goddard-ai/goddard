import { css } from "@goddard-ai/styled-system/css"
import { Popover, type ListNavigationController } from "@goddard-ai/ui-primitives"
import { useSignal } from "@preact/signals"
import { BookOpen, Command, File, Folder, LoaderCircle } from "lucide-react"
import { useEffect } from "preact/hooks"

import type { SessionInputMenuState } from "./input-menu-detection.ts"
import type { SessionInputSuggestion, SessionInputTrigger } from "./input.tsrx"
import {
  inputMenuBodyClass,
  inputMenuButtonClass,
  inputMenuClass,
  inputMenuContentClass,
  inputMenuDetailClass,
  inputMenuEmptyClass,
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

function SuggestionIcon(props: { suggestion: SessionInputSuggestion }) @{
  if (props.suggestion.type === "folder") {
    return <Folder size={14} strokeWidth={2.2} />
  }

  if (props.suggestion.type === "file") {
    return <File size={14} strokeWidth={2.2} />
  }

  if (props.suggestion.type === "skill") {
    return <BookOpen size={14} strokeWidth={2.2} />
  }

  <Command size={14} strokeWidth={2.2} />
}

export function SessionInputMenu(props: {
  isLoadingSuggestions: boolean;
  itemRef: ListNavigationController["itemRef"];
  menu: SessionInputMenuState | null;
  menuRef: (element: HTMLDivElement | null) => void;
  onAcceptSuggestion: (suggestion: SessionInputSuggestion) => void;
  onDismiss: () => void;
  suggestions: readonly SessionInputSuggestion[];
}) @{
  if (!props.menu) {
    return null
  }

  const menu = props.menu!
  const open = useSignal(Boolean(props.menu))

  useEffect(() => {
    open.value = Boolean(props.menu)
  }, [open, props.menu])

  <Popover
    anchor={() => ({
      x: menu.anchorLeft,
      y: menu.anchorTop,
    })}
    ariaLabel={getMenuHeading(menu.trigger)}
    blockOutsidePointer={false}
    class={inputMenuClass}
    closeOnEscape={true}
    closeOnOutsidePointer={true}
    focusOnOpen={false}
    offset={0}
    {open}
    placement="bottom-start"
    restoreFocus={false}
    role="presentation"
    strategy="fixed"
    onOpenChange={(nextOpen) => {
      if (!nextOpen) {
        props.onDismiss()
      }
    }}
  >
    <div ref={props.menuRef} class={inputMenuContentClass}>
      <div class={inputMenuHeaderClass}>{getMenuHeading(menu.trigger)}</div>
      <div class={inputMenuListClass} aria-label={getMenuHeading(menu.trigger)} role="listbox">
        @if (props.isLoadingSuggestions) {
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
        } @else if (props.suggestions.length === 0) {
          <div class={inputMenuEmptyClass}>No matches for this trigger yet.</div>
        } @else {
          @for (const suggestion of props.suggestions; index index) {
            <button
              ref={props.itemRef(index)}
              aria-selected="false"
              class={inputMenuButtonClass}
              key={suggestionKey(suggestion)}
              role="option"
              type="button"
              onMouseDown={(event) => {
                event.preventDefault()
              }}
              onClick={() => {
                props.onAcceptSuggestion(suggestion)
              }}
            >
              <span class={inputMenuIconClass} aria-hidden="true">
                <SuggestionIcon {suggestion} />
              </span>
              <span class={inputMenuBodyClass}>
                <span class={inputMenuLabelClass}>{getSuggestionLabel(suggestion)}</span>
                <span class={inputMenuDetailClass}>{getSuggestionDetail(suggestion)}</span>
              </span>
            </button>
          }
        }
      </div>
    </div>
  </Popover>
}
