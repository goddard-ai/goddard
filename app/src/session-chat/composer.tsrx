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
  footerControls: styles.footerControls,
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
  helperText?: preact.ComponentChildren;
  footerControls?: preact.ComponentChildren;
  contextUsage?: SessionInputContextUsage | null;
  promptHistory?: readonly SessionInputPromptBlocks[];
  submitLabel?: string;
}) {
  <SessionInput
    classes={composerClasses}
    contextUsage={props.contextUsage}
    disabled={props.disabled}
    footerInsideFrame={true}
    footerControls={props.footerControls}
    helperText={props.helperText}
    iconOnlySubmit={true}
    loadSuggestions={props.loadSuggestions}
    onPromptChange={props.onPromptChange}
    placeholder={props.placeholder}
    promptHistory={props.promptHistory}
    submitLabel={props.submitLabel}
    onSubmit={props.onSubmit}
  />
}
