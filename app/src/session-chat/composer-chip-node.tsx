/** Lexical decorator node and shared chip data used by the session chat composer. */
import { css, cx } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
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
import { BookOpen, Command, File, Folder } from "lucide-react"
import type { JSX } from "preact"

const CHIP_DATA_ATTRIBUTE = "data-goddard-composer-chip"

const chipClass = css({
  display: "inline-flex",
  alignItems: "center",
  gap: "8px",
  minHeight: "30px",
  paddingInline: "10px",
  borderRadius: "999px",
  border: "1px solid",
  borderColor: "border",
  background: `linear-gradient(180deg, ${token.var("colors.surface")} 0%, ${token.var("colors.background")} 100%)`,
  color: "text",
  boxShadow: `0 10px 20px color-mix(in srgb, ${token.var("colors.accent")} 10%, transparent)`,
})

const chipLabelClass = css({
  fontSize: "0.84rem",
  fontWeight: "680",
  lineHeight: "1.2",
})

const chipMutedClass = css({
  color: "muted",
})

/** Filesystem-backed chip data inserted for one `@` selection. */
export type ComposerFilesystemChipData = {
  kind: "file" | "folder"
  label: string
  path: string
  uri: string
  detail: string
}

/** Skill-backed chip data inserted for one `$` selection. */
export type ComposerSkillChipData = {
  kind: "skill"
  label: string
  path: string
  uri: string
  detail: string
  source: "local" | "global"
}

/** Slash-command chip data inserted for one `/` selection. */
export type ComposerSlashCommandChipData = {
  kind: "slash_command"
  label: string
  description: string
  inputHint: string | null
}

/** One atomic chip payload stored inside the Lexical session composer. */
export type ComposerChipData =
  | ComposerFilesystemChipData
  | ComposerSkillChipData
  | ComposerSlashCommandChipData

/** Serialized JSON shape used when Lexical copies or persists one chip node. */
export type SerializedComposerChipNode = SerializedLexicalNode & {
  chip: ComposerChipData
}

/** Returns the trigger text used when one chip falls back to plain text outside Lexical. */
export function chipTextFallback(chip: ComposerChipData) {
  if (chip.kind === "slash_command") {
    return `/${chip.label}`
  }

  return `${chip.kind === "skill" ? "$" : "@"}${chip.label}`
}

/** Returns true when one unknown value is valid serialized chip data. */
function isComposerChipData(value: unknown): value is ComposerChipData {
  if (typeof value !== "object" || value === null) {
    return false
  }

  const record = value as Record<string, unknown>

  if (record.kind === "file" || record.kind === "folder") {
    return (
      typeof record.label === "string" &&
      typeof record.path === "string" &&
      typeof record.uri === "string" &&
      typeof record.detail === "string"
    )
  }

  if (record.kind === "skill") {
    return (
      typeof record.label === "string" &&
      typeof record.path === "string" &&
      typeof record.uri === "string" &&
      typeof record.detail === "string" &&
      (record.source === "local" || record.source === "global")
    )
  }

  if (record.kind === "slash_command") {
    return (
      typeof record.label === "string" &&
      typeof record.description === "string" &&
      (typeof record.inputHint === "string" || record.inputHint === null)
    )
  }

  return false
}

/** Restores one chip node from copied HTML when the source editor is another Goddard composer. */
function importComposerChipFromElement(element: HTMLElement) {
  const rawChip = element.getAttribute(CHIP_DATA_ATTRIBUTE)

  if (!rawChip) {
    return null
  }

  try {
    const parsedChip = JSON.parse(rawChip) as unknown

    if (!isComposerChipData(parsedChip)) {
      return null
    }

    return {
      node: $createComposerChipNode(parsedChip),
    }
  } catch {
    return null
  }
}

/** Renders the shared icon used by one visible composer chip. */
function ChipIcon(props: { chip: ComposerChipData }) {
  if (props.chip.kind === "folder") {
    return <Folder size={14} strokeWidth={2.2} />
  }

  if (props.chip.kind === "file") {
    return <File size={14} strokeWidth={2.2} />
  }

  if (props.chip.kind === "skill") {
    return <BookOpen size={14} strokeWidth={2.2} />
  }

  return <Command size={14} strokeWidth={2.2} />
}

/** Visual chip surface rendered inline by the Lexical decorator node. */
function ComposerChipView(props: { chip: ComposerChipData }) {
  return (
    <span class={chipClass} data-chip-kind={props.chip.kind}>
      <span class={cx(chipMutedClass)} aria-hidden="true">
        <ChipIcon chip={props.chip} />
      </span>
      <span class={chipLabelClass}>
        {props.chip.kind === "slash_command" ? `/${props.chip.label}` : props.chip.label}
      </span>
    </span>
  )
}

/** Inline Lexical decorator node that stores one atomic composer chip. */
export class ComposerChipNode extends DecoratorNode<JSX.Element> {
  __chip: ComposerChipData

  static getType() {
    return "session-composer-chip"
  }

  static clone(node: ComposerChipNode) {
    return new ComposerChipNode(node.__chip, node.__key)
  }

  static importJSON(serializedNode: SerializedComposerChipNode) {
    return $createComposerChipNode(serializedNode.chip)
  }

  static importDOM(): DOMConversionMap | null {
    return {
      span: (element) => {
        if (!(element instanceof HTMLElement) || !element.hasAttribute(CHIP_DATA_ATTRIBUTE)) {
          return null
        }

        return {
          conversion: importComposerChipFromElement,
          priority: 4,
        }
      },
    }
  }

  constructor(chip: ComposerChipData, key?: NodeKey) {
    super(key)
    this.__chip = chip
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
    element.setAttribute(CHIP_DATA_ATTRIBUTE, JSON.stringify(this.__chip))
    element.textContent = chipTextFallback(this.__chip)
    return { element }
  }

  exportJSON(): SerializedComposerChipNode {
    return {
      ...super.exportJSON(),
      chip: this.__chip,
      type: ComposerChipNode.getType(),
      version: 1,
    }
  }

  getChip() {
    return this.getLatest().__chip
  }

  getTextContent() {
    return chipTextFallback(this.getLatest().__chip)
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
    return <ComposerChipView chip={this.getChip()} />
  }
}

/** Creates one inline chip node and registers it in the active Lexical update. */
export function $createComposerChipNode(chip: ComposerChipData) {
  return $applyNodeReplacement(new ComposerChipNode(chip))
}

/** Narrows one Lexical node to a session composer chip node when possible. */
export function $isComposerChipNode(
  node: LexicalNode | null | undefined,
): node is ComposerChipNode {
  return node instanceof ComposerChipNode
}
