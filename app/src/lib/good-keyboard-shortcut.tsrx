import { cx } from "@goddard-ai/styled-system/css"

import styles from "./good-keyboard-shortcut.style.ts"

type ShortcutPart = {
  key: string;
  label: string;
  kind: "character" | "symbol" | "named";
}

const shortcutSymbolLabels: Record<string, string> = {
  alt: "⌥",
  arrowdown: "↓",
  arrowleft: "←",
  arrowright: "→",
  arrowup: "↑",
  backspace: "⌫",
  capslock: "⇪",
  cmd: "⌘",
  command: "⌘",
  control: "⌃",
  ctrl: "⌃",
  del: "⌦",
  delete: "⌦",
  down: "↓",
  eject: "⏏",
  enter: "↩",
  esc: "⎋",
  escape: "⎋",
  left: "←",
  mod: "⌘",
  option: "⌥",
  pagedown: "⇟",
  pageup: "⇞",
  return: "↩",
  right: "→",
  shift: "⇧",
  tab: "⇥",
  up: "↑",
}

const shortcutNonMacModifierLabels: Record<string, string> = {
  alt: "Alt",
  cmd: "Cmd",
  command: "Cmd",
  control: "Ctrl",
  ctrl: "Ctrl",
  mod: "Ctrl",
  option: "Alt",
  shift: "Shift",
}

const shortcutNamedLabels: Record<string, string> = {
  end: "End",
  fn: "fn",
  home: "Home",
  space: "Space",
}

function formatCharacterKey(key: string) {
  return key.length === 1 && key.toLowerCase() !== key.toUpperCase() ? key.toUpperCase() : key
}

function getDigitKeyLabel(key: string) {
  const digitMatch = /^Digit([0-9])$/i.exec(key)

  return digitMatch?.[1]
}

function isMacOsPlatform() {
  return (globalThis.navigator as Navigator & {
      userAgentData: { platform: string };
    }).userAgentData.platform === "macOS"
}

function getShortcutPart(key: string): ShortcutPart {
  const normalizedKey = key.toLowerCase()
  const nonMacModifierLabel = isMacOsPlatform()
    ? undefined
    : shortcutNonMacModifierLabels[normalizedKey]

  if (nonMacModifierLabel) {
    return {
      key,
      label: nonMacModifierLabel,
      kind: "named",
    }
  }

  const symbolLabel = shortcutSymbolLabels[normalizedKey]

  if (symbolLabel) {
    return {
      key,
      label: symbolLabel,
      kind: "symbol",
    }
  }

  const namedLabel = shortcutNamedLabels[normalizedKey]

  if (namedLabel) {
    return {
      key,
      label: namedLabel,
      kind: "named",
    }
  }

  const digitLabel = getDigitKeyLabel(key)

  if (digitLabel) {
    return {
      key,
      label: digitLabel,
      kind: "character",
    }
  }

  return {
    key,
    label: formatCharacterKey(key),
    kind: key.length === 1 ? "character" : "named",
  }
}

function getShortcutPartClass(part: ShortcutPart) {
  switch (part.kind) {
    case "character":
      return styles.characterKey
    case "symbol":
      return styles.symbolKey
    case "named":
      return styles.namedKey
  }
}

function parseShortcutExpression(expression: string) {
  return expression.trim().split(/\s+/).filter(Boolean).map(
    (combo) => combo.split("+").filter(Boolean).map(getShortcutPart),
  )
}

/** Renders one canonical shortcut expression with macOS key symbols and monospace character keys. */
export function GoodKeyboardShortcut(props: {
  expression: string;
  class?: string;
  "aria-label"?: string;
}) @{
  const combos = parseShortcutExpression(props.expression)

  <span
    aria-label={props["aria-label"] ?? props.expression}
    class={cx(styles.root, props.class)}
    title={props.expression}
  >
    @for (const combo of combos; index comboIndex) {
      <>
        @if (comboIndex > 0) {
          <span aria-hidden={true} class={styles.sequenceSeparator}>then</span>
        }

        @for (const part of combo) {
          <span
            aria-hidden={true}
            class={getShortcutPartClass(part)}
            data-key-kind={part.kind}
            key={`${comboIndex}:${part.key}`}
          >{part.label}</span>
        }
      </>
    }
  </span>
}
