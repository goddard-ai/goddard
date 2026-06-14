import type {
  DOMConversionMap,
  DOMExportOutput,
  EditorConfig,
  LexicalEditor,
  LexicalNode,
  NodeKey,
  SerializedLexicalNode,
} from "lexical"
import { $applyNodeReplacement, DecoratorNode } from "lexical"

import type { ComposerChipDecoratorNodeConstructor } from "./composer-chip-node-types.ts"
import styles from "./composer-shell-prompt-node.style.ts"

const SHELL_PROMPT_DATA_ATTRIBUTE = "data-goddard-composer-shell-prompt"
const SHELL_PROMPT_TEXT = "$ "

export type SerializedComposerShellPromptNode = SerializedLexicalNode

function importComposerShellPromptFromElement(element: HTMLElement) {
  if (!element.hasAttribute(SHELL_PROMPT_DATA_ATTRIBUTE)) {
    return null
  }

  return {
    node: $createComposerShellPromptNode(),
  }
}

const ComposerShellPromptDecoratorNode = DecoratorNode as ComposerChipDecoratorNodeConstructor

function ComposerShellPromptView() @{
  <span aria-label="Shell prompt" class={styles.prompt}>
    $
  </span>
}

/** Inline Lexical decorator node that marks one shell-command line inside the composer. */
export class ComposerShellPromptNode extends ComposerShellPromptDecoratorNode {
  static getType() {
    return "session-composer-shell-prompt"
  }

  static clone(node: ComposerShellPromptNode) {
    return new ComposerShellPromptNode(node.__key)
  }

  static importJSON(_serializedNode: SerializedComposerShellPromptNode) {
    return $createComposerShellPromptNode()
  }

  static importDOM(): DOMConversionMap | null {
    return {
      span: (element) => {
        if (
          !(element instanceof HTMLElement) || !element.hasAttribute(SHELL_PROMPT_DATA_ATTRIBUTE)
        ) {
          return null
        }

        return {
          conversion: importComposerShellPromptFromElement,
          priority: 4,
        }
      },
    }
  }

  createDOM(_config: EditorConfig) {
    const element = document.createElement("span")
    element.contentEditable = "false"
    return element
  }

  updateDOM() {
    return false
  }

  exportDOM(_editor: LexicalEditor): DOMExportOutput {
    const element = document.createElement("span")
    element.contentEditable = "false"
    element.setAttribute(SHELL_PROMPT_DATA_ATTRIBUTE, "true")
    element.textContent = SHELL_PROMPT_TEXT
    return { element }
  }

  exportJSON(): SerializedComposerShellPromptNode {
    return {
      ...super.exportJSON(),
      type: ComposerShellPromptNode.getType(),
      version: 1,
    }
  }

  getTextContent() {
    return SHELL_PROMPT_TEXT
  }

  isInline() {
    return true
  }

  isIsolated() {
    return true
  }

  isKeyboardSelectable() {
    return true
  }

  decorate() {
    return <ComposerShellPromptView />
  }
}

/** Creates one inline shell-prompt node and registers it in the active Lexical update. */
export function $createComposerShellPromptNode() {
  return $applyNodeReplacement(new ComposerShellPromptNode())
}

/** Checks whether one Lexical node is implemented by the session composer shell prompt class. */
export function $isComposerShellPromptNode(node: LexicalNode | null | undefined) {
  return node instanceof ComposerShellPromptNode
}
