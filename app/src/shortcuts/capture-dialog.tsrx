import { type Signal } from "@preact/signals"
import { X } from "lucide-react"
import type { RecordingSession } from "powerkeys"
import { useEffect, useId, useRef, useState } from "preact/hooks"

import { useShortcutRegistry } from "~/app-state-context.tsrx"
import { text } from "~/language/text.ts"
import { GoodKeyboardShortcut } from "~/lib/good-keyboard-shortcut.tsrx"
import { Modal } from "@goddard-ai/ui-primitives"
import styles from "./capture-dialog.style.ts"

export function KeyboardShortcutCaptureDialog(props: {
  open: Signal<boolean>;
  label: string;
  mode: "add" | "change";
  currentExpression: string | null;
  whenClause: string | null;
  onRemove?: () => Promise<void> | void;
  onSave: (expression: string) => Promise<void> | void;
}) @{
  const shortcutRegistry = useShortcutRegistry()
  const sessionRef = useRef<RecordingSession | null>(null)
  const titleId = useId()
  const [draftExpression, setDraftExpression] = useState(props.currentExpression ?? "")
  const [isRecording, setIsRecording] = useState(false)

  useEffect(() => {
    if (!props.open.value) {
      setDraftExpression(props.currentExpression ?? "")
      setIsRecording(false)
      return
    }

    let isDisposed = false
    setDraftExpression(props.currentExpression ?? "")

    function beginCapture() {
      if (isDisposed) {
        return
      }

      try {
        setIsRecording(true)

        const session = shortcutRegistry.runtime.record({
          suppressHandlers: true,
          consumeEvents: true,
          onUpdate: (recording) => {
            if (!isDisposed) {
              setDraftExpression(recording.expression)
            }
          },
        })

        sessionRef.current = session

        void session.finished.then(
          (recording) => {
            if (sessionRef.current === session) {
              sessionRef.current = null
            }
            if (isDisposed) {
              return
            }

            setDraftExpression(recording.expression)
            beginCapture()
          },
          () => {
            if (sessionRef.current === session) {
              sessionRef.current = null
            }
            if (!isDisposed) {
              setIsRecording(false)
            }
          },
        )
      } catch (error) {
        setIsRecording(false)
        console.error("Failed to begin shortcut capture.", error)
      }
    }

    beginCapture()

    return () => {
      isDisposed = true
      setIsRecording(false)
      const session = sessionRef.current
      sessionRef.current = null
      session?.cancel()
    }
  }, [props.currentExpression, props.open.value, shortcutRegistry.runtime])

  <Modal
    open={props.open}
    {titleId}
    backdropClass={styles.backdrop}
    positionerClass={styles.positioner}
    contentClass={styles.content}
  >
    <h2 id={titleId} class={styles.title}>
      <span class={styles.titleLabel}>
        {props.mode === "add" ? text.addShortcut : text.changeShortcut}
      </span>
      <span class={styles.titleText}>{props.label}</span>
    </h2>
    <button
      class={styles.closeButton}
      type="button"
      onClick={() => {
        props.open.value = false
      }}
    >
      <X size={16} strokeWidth={2.2} />
    </button>
    <dl class={styles.metaList}>
      @if (props.currentExpression) {
        <div class={styles.metaRow}>
          <dt class={styles.metaTerm}>{text.currentShortcut}</dt>
          <dd class={styles.metaDetail}>
            <GoodKeyboardShortcut expression={props.currentExpression} />
          </dd>
        </div>
      }
      @if (props.whenClause) {
        <div class={styles.metaRow}>
          <dt class={styles.metaTerm}>{text.when}</dt>
          <dd class={styles.metaDetail}>{props.whenClause}</dd>
        </div>
      }
    </dl>
    <section class={styles.captureSection}>
      <div class={styles.captureHeading}>
        <span class={styles.captureLabel}>{text.pressTheShortcutYouWantToUse}</span>

        @if (isRecording) {
          <span class={styles.captureStatus}>{text.listening}</span>
        }
      </div>
      <div class={styles.captureValue}>
        @if (draftExpression) {
          <GoodKeyboardShortcut expression={draftExpression} />
        } @else {
          <span class={styles.capturePlaceholder}>{text.typeAKeyCombinationOrSequence}</span>
        }
      </div>
    </section>
    <div class={styles.actionRow}>
      <button
        class={styles.actionButton}
        type="button"
        onClick={() => {
          props.open.value = false
        }}
      >{text.cancel}</button>

      @if (props.onRemove) {
        <button
          class={`${styles.actionButton} ${styles.actionButtonDanger}`}
          type="button"
          onClick={() => {
            void props.onRemove?.()
          }}
        >{text.removeShortcut}</button>
      }

      <button
        class={`${styles.actionButton} ${styles.actionButtonPrimary}`}
        disabled={draftExpression.trim().length === 0}
        type="button"
        onClick={() => {
          void props.onSave(draftExpression.trim())
        }}
      >{text.saveShortcut}</button>
    </div>
  </Modal>
}
