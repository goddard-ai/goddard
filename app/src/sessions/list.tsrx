import type { DaemonSession } from "@goddard-ai/sdk"

import { text } from "~/language/text.ts"
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
    return <div class={styles.loading}>{text.loadingSessions}</div>
  }

  if (props.listStatus === "error" && props.sessions.length === 0) {
    return <div class={styles.error}>
      <div class={styles.errorContent}>
        <h2 class={styles.title}>{text.couldntLoadSessions}</h2>
        <p class={styles.body}>
          {props.errorMessage ?? text.theDaemonBackedSessionListRequestFailed}
        </p>
      </div>
    </div>
  }

  if (props.sessions.length === 0) {
    return <div class={styles.empty}>
      <div class={styles.emptyContent}>
        <h2 class={styles.title}>
          {props.searchQuery.trim().length > 0 ? text.noMatchingSessions : text.noSessionsYet}
        </h2>
        <p class={styles.description}>
          {props.searchQuery.trim().length > 0
            ? text.tryAShorterTitleQueryOrDifferentKeywords
            : text.sessionsWillAppearHereOnceTheDaemonHasCreatedThem}
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
