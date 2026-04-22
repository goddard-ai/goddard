import type { DaemonSession } from "@goddard-ai/sdk"

import { useQuery } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"
import styles from "./view.style.ts"

component SessionChangesView(props: {
  repositoryLabel: string;
  sessionId: DaemonSession["id"];
  sessionTitle: string;
}) {
  const changes = useQuery(goddardSdk.session.changes, [
    { id: props.sessionId },
  ])

  <section class={styles.root}>
    <header class={styles.header}>
      <p class={styles.repository}>
        {props.repositoryLabel}
      </p>
      <h1 class={styles.title}>
        {props.sessionTitle}
      </h1>
      <p class={styles.description}>{"Current git diff for this session workspace."}</p>
      {changes.workspaceRoot
        ? <tsx>
            <p class={styles.workspaceRoot}>
              {changes.workspaceRoot}
            </p>
          </tsx>
        : null}
    </header>
    {changes.workspaceRoot === null
      ? <tsx>
          <div class={styles.empty}>
            <h2 class={styles.emptyTitle}>
              {"Git diff unavailable"}
            </h2>
            <p class={styles.description}>
              {"This session is not currently attached to a git workspace."}
            </p>
          </div>
        </tsx>
      : changes.hasChanges
        ? <tsx>
            <div class={styles.panel}>
              <div class={styles.diffViewport}>
                <pre class={styles.diff}>
                  {changes.diff}
                </pre>
              </div>
            </div>
          </tsx>
        : <tsx>
            <div class={styles.empty}>
              <h2 class={styles.emptyTitle}>
                {"No changes"}
              </h2>
              <p class={styles.description}>
                {"This session workspace is currently clean."}
              </p>
            </div>
          </tsx>}
  </section>
}

export default SessionChangesView
