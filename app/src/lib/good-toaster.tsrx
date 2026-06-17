import { signal } from "@preact/signals"
import { OverlayPortal } from "@goddard-ai/ui-primitives"
import { LoaderCircle, X } from "lucide-react"

import { text } from "~/language/text.ts"
import styles from "./good-toaster.style.ts"

type ToastType = "success" | "info" | "error" | "loading"

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

function resolveToastDuration(input: Pick<ToastInput, "duration" | "type">) {
  if (input.duration !== undefined) {
    return input.duration
  }

  return input.type === "error" || input.type === "loading" ? null : 4000
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

  function applyToastLimit(nextToasts: ToastRecord[]) {
    while (nextToasts.length > options.max) {
      const toastToDismiss = findToastToDismiss(nextToasts)
      if (!toastToDismiss) {
        break
      }

      clearToastTimer(toastToDismiss.id)
      nextToasts.splice(nextToasts.indexOf(toastToDismiss), 1)
    }

    return nextToasts
  }

  return {
    toasts,
    create(input: ToastInput) {
      const toast = createToastRecord(input)
      toasts.value = applyToastLimit([...toasts.value, toast])
      scheduleDismiss(toast)
      return toast.id
    },
    update(id: string, input: Omit<ToastInput, "id">) {
      const toast = createToastRecord({
        ...input,
        id,
      })
      const toastIndex = toasts.value.findIndex((candidate) => candidate.id === id)

      clearToastTimer(id)

      if (toastIndex === -1) {
        toasts.value = applyToastLimit([...toasts.value, toast])
      } else {
        toasts.value = toasts.value.map(
          (candidate, index) => index === toastIndex ? toast : candidate,
        )
      }

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

export function GoodToaster() @{
  <OverlayPortal root="menu">
    <div aria-live="polite" aria-relevant="additions text" class={styles.group}>
      @for (const toast of appToaster.toasts.value) {
        <div key={toast.id} class={styles.root} data-state="open" data-type={toast.type}>
          @if (toast.type === "loading") {
            <LoaderCircle aria-hidden="true" class={styles.loadingIcon} size={16} />
          } @else {
            <span aria-hidden="true" class={styles.accent} />
          }

          <div class={styles.body}>
            <span class={styles.title}>{toast.title}</span>

            @if (toast.description != null) {
              <span class={styles.description}>{toast.description}</span>
            }
          </div>
          <button
            aria-label={text.dismissNotification}
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
