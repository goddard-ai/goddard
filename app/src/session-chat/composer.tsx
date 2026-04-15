import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"

import {
  SessionInput,
  type SessionInputClasses,
  type SessionInputPromptBlocks,
  type SessionInputSuggestionLoader,
} from "~/session-input/input.tsx"

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

const composerClasses = {
  form: composerFormClass,
  editorFrame: editorFrameClass,
  contentEditable: contentEditableClass,
  placeholder: placeholderClass,
  footer: footerClass,
  helperText: helperTextClass,
  submitButton: submitButtonClass,
} satisfies SessionInputClasses

export function Composer(props: {
  loadSuggestions: SessionInputSuggestionLoader
  onSubmit: (prompt: SessionInputPromptBlocks) => Promise<void> | void
  onPromptChange?: (prompt: SessionInputPromptBlocks) => void
  placeholder?: string
  helperText?: string
  submitLabel?: string
}) {
  return (
    <SessionInput
      classes={composerClasses}
      helperText={props.helperText}
      loadSuggestions={props.loadSuggestions}
      onPromptChange={props.onPromptChange}
      placeholder={props.placeholder}
      submitLabel={props.submitLabel}
      onSubmit={props.onSubmit}
    />
  )
}
