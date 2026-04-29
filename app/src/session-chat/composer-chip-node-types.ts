import type { DecoratorNode } from "lexical"
import type { JSX } from "preact"

/** Constructor view used to instantiate Lexical's generic decorator node from TSRX. */
export type ComposerChipDecoratorNodeConstructor = typeof DecoratorNode<JSX.Element>
