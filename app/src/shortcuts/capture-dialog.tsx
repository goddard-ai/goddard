import { Dialog, type UseDialogReturn } from "@ark-ui/react/dialog"
import { Portal } from "@ark-ui/react/portal"
import { X } from "lucide-react"
import type { RecordingSession } from "powerkeys"
import { useEffect, useRef, useState } from "preact/hooks"

import { useShortcutRegistry } from "~/app-state-context.tsx"
import styles from "./capture-dialog.style.ts"

export function KeyboardShortcutCaptureDialog(props: {
  dialog: UseDialogReturn
  label: string
  mode: "add" | "change"
  currentExpression: string | null
  whenClause: string | null
  onRemove?: () => Promise<void> | void
  onSave: (expression: string) => Promise<void> | void
}) {
  const shortcutRegistry = useShortcutRegistry()
  const sessionRef = useRef<RecordingSession | null>(null)
  const [draftExpression, setDraftExpression] = useState(props.currentExpression ?? "")
  const [isRecording, setIsRecording] = useState(false)

  useEffect(() => {
    if (!props.dialog.open) {
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
  }, [props.currentExpression, props.dialog.open, shortcutRegistry.runtime])

  return (
    <Portal>
      <Dialog.Backdrop class={styles.backdrop} />
      <Dialog.Positioner class={styles.positioner}>
        <Dialog.Content class={styles.content}>
          <Dialog.Title class={styles.title}>
            <span class={styles.titleLabel}>
              {props.mode === "add" ? "Add shortcut" : "Change shortcut"}
            </span>
            <span class={styles.titleText}>{props.label}</span>
          </Dialog.Title>

          <Dialog.CloseTrigger asChild>
            <button class={styles.closeButton} type="button">
              <X size={16} strokeWidth={2.2} />
            </button>
          </Dialog.CloseTrigger>

          <dl class={styles.metaList}>
            {props.currentExpression ? (
              <div class={styles.metaRow}>
                <dt class={styles.metaTerm}>Current shortcut</dt>
                <dd class={styles.metaDetail}>{props.currentExpression}</dd>
              </div>
            ) : null}

            {props.whenClause ? (
              <div class={styles.metaRow}>
                <dt class={styles.metaTerm}>When</dt>
                <dd class={styles.metaDetail}>{props.whenClause}</dd>
              </div>
            ) : null}
          </dl>

          <section class={styles.captureSection}>
            <div class={styles.captureHeading}>
              <span class={styles.captureLabel}>Press the shortcut you want to use</span>
              {isRecording ? <span class={styles.captureStatus}>Listening…</span> : null}
            </div>

            <div class={styles.captureValue}>
              {draftExpression ? (
                draftExpression
              ) : (
                <span class={styles.capturePlaceholder}>Type a key combination or sequence</span>
              )}
            </div>
          </section>

          <div class={styles.actionRow}>
            <Dialog.CloseTrigger asChild>
              <button class={styles.actionButton} type="button">
                Cancel
              </button>
            </Dialog.CloseTrigger>

            {props.onRemove ? (
              <button
                class={`${styles.actionButton} ${styles.actionButtonDanger}`}
                type="button"
                onClick={() => {
                  void props.onRemove?.()
                }}
              >
                Remove shortcut
              </button>
            ) : null}

            <button
              class={`${styles.actionButton} ${styles.actionButtonPrimary}`}
              disabled={draftExpression.trim().length === 0}
              type="button"
              onClick={() => {
                void props.onSave(draftExpression.trim())
              }}
            >
              Save shortcut
            </button>
          </div>
        </Dialog.Content>
      </Dialog.Positioner>
    </Portal>
  )
}

export default KeyboardShortcutCaptureDialog
