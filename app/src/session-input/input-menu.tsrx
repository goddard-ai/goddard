import { css, cx } from "@goddard-ai/styled-system/css"
import { BookOpen, Command, File, Folder, LoaderCircle } from "lucide-react"

import { MenuPortal } from "~/lib/menu-portal.tsrx"
import type { SessionInputMenuState } from "./input-lexical.tsrx"
import type { SessionInputSuggestion, SessionInputTrigger } from "./input.tsrx"
import {
  inputMenuBodyClass,
  inputMenuButtonActiveClass,
  inputMenuButtonClass,
  inputMenuClass,
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

component SuggestionIcon(props: { suggestion: SessionInputSuggestion }) {
  if (props.suggestion.type === "folder") {
    <Folder size={14} strokeWidth={2.2} />
    return
  }

  if (props.suggestion.type === "file") {
    <File size={14} strokeWidth={2.2} />
    return
  }

  if (props.suggestion.type === "skill") {
    <BookOpen size={14} strokeWidth={2.2} />
    return
  }

  <Command size={14} strokeWidth={2.2} />
}

export component SessionInputMenu(props: {
  isLoadingSuggestions: boolean;
  menu: SessionInputMenuState | null;
  menuRef: preact.RefObject<HTMLDivElement | null>;
  onAcceptSuggestion: (suggestion: SessionInputSuggestion) => void;
  onHighlight: (index: number) => void;
  selectedIndex: number;
  suggestions: readonly SessionInputSuggestion[];
}) {
  if (!props.menu) {
    return
  }

  <MenuPortal>
    <div
      ref={props.menuRef}
      class={inputMenuClass}
      style={{
        left: `${props.menu.anchorLeft}px`,
        top: `${props.menu.anchorTop}px`,
      }}
    >
      <div class={inputMenuHeaderClass}>
        {getMenuHeading(props.menu.trigger)}
      </div>
      <div class={inputMenuListClass}>
        {props.isLoadingSuggestions
          ? <tsx:react>
              <div class={inputMenuEmptyClass}>
                <span class={css(
                  {
                    display: "inline-flex",
                    alignItems: "center",
                    gap: "8px",
                  },
                )}>
                  <LoaderCircle class={css({ animation: "spin 1s linear infinite" })} size={14} />
                  Loading suggestions...
                </span>
              </div>
            </tsx:react>
          : props.suggestions.length === 0
            ? <tsx:react>
                <div class={inputMenuEmptyClass}>No matches for this trigger yet.</div>
              </tsx:react>
            : (props.suggestions.map(
                (suggestion, index) => <tsx:react>
                  <button key={suggestionKey(suggestion)} class={cx(
                    inputMenuButtonClass,
                    index === props.selectedIndex ? inputMenuButtonActiveClass : undefined,
                  )} type="button" onMouseDown={(event) => {
                    event.preventDefault()
                  }} onMouseEnter={() => {
                    props.onHighlight(index)
                  }} onClick={() => {
                    props.onAcceptSuggestion(suggestion)
                  }}>
                    <span class={inputMenuIconClass} aria-hidden="true">
                      <SuggestionIcon suggestion={suggestion} />
                    </span>
                    <span class={inputMenuBodyClass}>
                      <span class={inputMenuLabelClass}>
                        {getSuggestionLabel(suggestion)}
                      </span>
                      <span class={inputMenuDetailClass}>
                        {getSuggestionDetail(suggestion)}
                      </span>
                    </span>
                  </button>
                </tsx:react>,
              ))}
      </div>
    </div>
  </MenuPortal>
}
