import { signal } from "@preact/signals"
import { OverlayPortal } from "@goddard-ai/ui-primitives"
import { X } from "lucide-react"

import styles from "./good-toaster.style.ts"

type ToastType = "success" | "info" | "error"

type ToastInput = {
  id?: string;
  title: preact.ComponentChildren;
  description?: preact.ComponentChildren;
  type?: ToastType;
  duration?: number | null;
}

type ToastRecord = {
  id: string;
  title: preact.ComponentChildren;
  description: preact.ComponentChildren;
  type: ToastType;
  duration: number | null;
}

type ToasterOptions = {
  max: number;
}

let nextToastId = 0

function resolveToastDuration(input: ToastInput) {
  if (input.duration !== undefined) {
    return input.duration
  }

  return input.type === "error" ? null : 4000
}

function createToastRecord(input: ToastInput): ToastRecord {
  return {
    id: input.id ?? `toast-${++nextToastId}`,
    title: input.title,
    description: input.description,
    type: input.type ?? "info",
    duration: resolveToastDuration(input),
  }
}

function findToastToDismiss(toasts: readonly ToastRecord[]) {
  return toasts.find((toast) => toast.type !== "error") ?? toasts[0] ?? null
}

/** Creates one signal-backed toast store with Ark-compatible app call sites. */
export function createToaster(options: ToasterOptions) {
  const toasts = signal<ToastRecord[]>([])
  const timers = new Map<string, ReturnType<typeof window.setTimeout>>()

  function clearToastTimer(id: string) {
    const timer = timers.get(id)
    if (timer !== undefined) {
      window.clearTimeout(timer)
      timers.delete(id)
    }
  }

  function dismiss(id: string) {
    clearToastTimer(id)
    toasts.value = toasts.value.filter((toast) => toast.id !== id)
  }

  function scheduleDismiss(toast: ToastRecord) {
    if (toast.duration === null) {
      return
    }

    timers.set(
      toast.id,
      window.setTimeout(() => {
        dismiss(toast.id)
      }, toast.duration),
    )
  }

  return {
    toasts,
    create(input: ToastInput) {
      const toast = createToastRecord(input)
      const nextToasts = [...toasts.value, toast]

      while (nextToasts.length > options.max) {
        const toastToDismiss = findToastToDismiss(nextToasts)
        if (!toastToDismiss) {
          break
        }

        clearToastTimer(toastToDismiss.id)
        nextToasts.splice(nextToasts.indexOf(toastToDismiss), 1)
      }

      toasts.value = nextToasts
      scheduleDismiss(toast)
      return toast.id
    },
    dismiss,
    dismissAll() {
      for (const toast of toasts.value) {
        clearToastTimer(toast.id)
      }

      toasts.value = []
    },
  }
}

export const appToaster = createToaster({
  max: 4,
})

export component GoodToaster() {
  <OverlayPortal root="menu">
    <div aria-live="polite" aria-relevant="additions text" class={styles.group}>
      for (const toast of appToaster.toasts.value) {
        <div key={toast.id} class={styles.root} data-state="open" data-type={toast.type}>
          <span aria-hidden="true" class={styles.accent} />
          <div class={styles.body}>
            <span class={styles.title}>
              {toast.title}
            </span>
            if (toast.description != null) {
              <span class={styles.description}>
                {toast.description}
              </span>
            }
          </div>
          <button
            aria-label="Dismiss notification"
            class={styles.closeButton}
            type="button"
            onClick={() => {
              appToaster.dismiss(toast.id)
            }}
          >
            <X size={15} strokeWidth={2.2} />
          </button>
        </div>
      }
    </div>
  </OverlayPortal>
}
