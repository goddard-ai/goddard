import {
  SessionInput,
  type SessionInputClasses,
  type SessionInputContextUsage,
  type SessionInputPromptBlocks,
  type SessionInputSubmitMode,
  type SessionInputSuggestionLoader,
} from "~/session-input/input.tsrx"
import styles from "~/session-input/composer-layout.style.ts"

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

export function Composer(props: {
  disabled?: boolean;
  loadSuggestions: SessionInputSuggestionLoader;
  onSubmit: (prompt: SessionInputPromptBlocks, options: {
    mode: SessionInputSubmitMode;
  }) => Promise<void> | void;
  onPromptChange?: (prompt: SessionInputPromptBlocks) => void;
  placeholder?: string;
  helperText?: preact.ComponentChildren;
  footerControls?: preact.ComponentChildren;
  contextUsage?: SessionInputContextUsage | null;
  submitLabel?: string;
  enableQueueSubmitShortcut?: boolean;
}) @{
  <SessionInput
    classes={composerClasses}
    contextUsage={props.contextUsage}
    disabled={props.disabled}
    footerInsideFrame={true}
    footerControls={props.footerControls}
    helperText={props.helperText}
    iconOnlySubmit={true}
    activationFocus={true}
    enableQueueSubmitShortcut={props.enableQueueSubmitShortcut}
    loadSuggestions={props.loadSuggestions}
    onPromptChange={props.onPromptChange}
    placeholder={props.placeholder}
    submitLabel={props.submitLabel}
    onSubmit={props.onSubmit}
  />
}
