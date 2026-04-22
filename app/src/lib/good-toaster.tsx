import { Portal } from "@ark-ui/react/portal";
import { Toast, Toaster, createToaster } from "@ark-ui/react/toast";
import { X } from "lucide-react";

import styles from "./good-toaster.style.ts";

export const appToaster = createToaster({
  gap: 12,
  max: 4,
  placement: "bottom-end",
});

export function GoodToaster() {
  return (
    <Portal>
      <Toaster toaster={appToaster} class={styles.group}>
        {(toast) => (
          <Toast.Root key={toast.id} class={styles.root}>
            <span aria-hidden="true" class={styles.accent} />
            <div class={styles.body}>
              <Toast.Title class={styles.title}>{toast.title}</Toast.Title>
              {toast.description ? (
                <Toast.Description class={styles.description}>
                  {toast.description}
                </Toast.Description>
              ) : null}
            </div>
            <Toast.CloseTrigger class={styles.closeButton}>
              <X size={15} strokeWidth={2.2} />
            </Toast.CloseTrigger>
          </Toast.Root>
        )}
      </Toaster>
    </Portal>
  );
}
