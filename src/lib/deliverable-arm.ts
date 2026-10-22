import type { BossDeliverable } from '@waku/client';

import { createStore } from '@octane-xplat/ui/data';

/** The deliverable a row click armed the boss composer with: the record
 * snapshot the chip and sheet render, the boss chat it armed, and whether
 * its preview sheet is open. Mirrors the desktop's `command_deliverable` +
 * `deliverable_page` pair — a send or a session switch clears the arm. */
export interface DeliverableArm {
  deliverable: BossDeliverable;
  /** The publishing boss chat this arm belongs to. */
  sessionId: string;
  previewOpen: boolean;
}

export const deliverableArm = createStore<DeliverableArm | null>(null);

export function armDeliverable(arm: DeliverableArm): void {
  deliverableArm.set(arm);
}

export function clearDeliverableArm(): void {
  deliverableArm.set(null);
}

/** Re-open the armed deliverable's preview sheet (chip tap). */
export function showArmedDeliverablePreview(): void {
  deliverableArm.update((current) =>
    current ? { ...current, previewOpen: true } : current,
  );
}

export function dismissDeliverablePreview(): void {
  deliverableArm.update((current) =>
    current ? { ...current, previewOpen: false } : current,
  );
}
