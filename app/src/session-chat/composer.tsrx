import {
  SessionInput,
  type SessionInputClasses,
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
  submitButton: styles.submitButton,
}

export function Composer(props: {
  loadSuggestions: SessionInputSuggestionLoader;
  onSubmit: (prompt: SessionInputPromptBlocks) => Promise<void> | void;
  onPromptChange?: (prompt: SessionInputPromptBlocks) => void;
  placeholder?: string;
  helperText?: string;
  submitLabel?: string;
}) {
  return <tsx:react>
    <SessionInput classes={composerClasses} helperText={props.helperText} loadSuggestions={props.loadSuggestions} onPromptChange={props.onPromptChange} placeholder={props.placeholder} submitLabel={props.submitLabel} onSubmit={props.onSubmit} />
  </tsx:react>
}
