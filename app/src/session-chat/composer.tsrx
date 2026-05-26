import {
  SessionInput,
  type SessionInputClasses,
  type SessionInputContextUsage,
  type SessionInputPromptBlocks,
  type SessionInputSuggestionLoader,
} from "~/session-input/input.tsrx"
import styles from "./composer.style.ts"

const composerClasses: SessionInputClasses = {
  form: styles.form,
  editorFrame: styles.editorFrame,
  contentEditable: styles.contentEditable,
  placeholder: styles.placeholder,
  footer: styles.footer,
  helperText: styles.helperText,
  contextUsage: styles.contextUsage,
  submitButton: styles.submitButton,
}

export component Composer(props: {
  disabled?: boolean;
  loadSuggestions: SessionInputSuggestionLoader;
  onSubmit: (prompt: SessionInputPromptBlocks) => Promise<void> | void;
  onPromptChange?: (prompt: SessionInputPromptBlocks) => void;
  placeholder?: string;
  helperText?: string;
  contextUsage?: SessionInputContextUsage | null;
  submitLabel?: string;
}) {
  <SessionInput
    classes={composerClasses}
    contextUsage={props.contextUsage}
    disabled={props.disabled}
    helperText={props.helperText}
    loadSuggestions={props.loadSuggestions}
    onPromptChange={props.onPromptChange}
    placeholder={props.placeholder}
    submitLabel={props.submitLabel}
    onSubmit={props.onSubmit}
  />
}
