import { t } from "@lingui/core/macro"
import type { DaemonSession } from "@goddard-ai/sdk"

import { ListRow } from "./list-row.tsrx"
import styles from "./list.style.ts"

export function SessionsList(props: {
  errorMessage?: string | null;
  itemRef?: (index: number) => (element: HTMLElement | null) => void | (() => void);
  listStatus?: "idle" | "loading" | "ready" | "error";
  searchQuery: string;
  sessions: readonly DaemonSession[];
  showProjectNames?: boolean;
}) @{
  if (props.listStatus === "loading" && props.sessions.length === 0) {
    return <div class={styles.loading}>
      {t`Loading sessions...`}
    </div>
  }

  if (props.listStatus === "error" && props.sessions.length === 0) {
    return <div class={styles.error}>
      <div class={styles.errorContent}>
        <h2 class={styles.title}>
          {t`Couldn't load sessions`}
        </h2>
        <p class={styles.body}>
          {props.errorMessage ?? t`The daemon-backed session list request failed.`}
        </p>
      </div>
    </div>
  }

  if (props.sessions.length === 0) {
    return <div class={styles.empty}>
      <div class={styles.emptyContent}>
        <h2 class={styles.title}>
          {props.searchQuery.trim().length > 0 ? t`No matching sessions` : t`No sessions yet`}
        </h2>
        <p class={styles.description}>
          {props.searchQuery.trim().length > 0
            ? t`Try a shorter title query or different keywords.`
            : t`Sessions will appear here once the daemon has created them.`}
        </p>
      </div>
    </div>
  }

  <ul class={styles.list}>
    @for (const session of props.sessions; index index) {
      <li key={session.id}>
        <ListRow
          itemRef={props.itemRef?.(index)}
          {session}
          showProjectName={props.showProjectNames}
        />
      </li>
    }
  </ul>
}
