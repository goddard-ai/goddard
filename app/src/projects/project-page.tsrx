import type { InboxItem } from "@goddard-ai/inbox/schema"
import type { DaemonSession } from "@goddard-ai/sdk"
import type { ProjectGitCheckoutSummary } from "~/shared/desktop-rpc.ts"
import { cx } from "@goddard-ai/styled-system/css"
import { Archive, CircleDot, FileDiff, GitBranch, Inbox, MessageSquare } from "lucide-react"

import { useProjectRegistry, useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { getProjectGitStatus } from "~/desktop-host.ts"
import { getInboxItemPrimaryText, getInboxItemSecondaryText } from "~/inbox/text.ts"
import { useQueries } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"
import { getSessionDisplayTitle, getSessionRepositoryLabel } from "~/sessions/display.ts"
import { evictSessionHistory } from "~/sessions/mutations.ts"
import { findNearestProjectPath } from "./project-context.ts"
import styles from "./project-page.style.ts"

const PROJECT_DASHBOARD_SESSION_LIMIT = 50
const PROJECT_DASHBOARD_INBOX_LIMIT = 50

type SessionWithWorktree = {
  session: DaemonSession;
  worktree: ProjectSessionWorktree | null;
}

type ProjectSessionWorktree = {
  repoRoot: string;
}

function formatCountLabel(count: number, singular: string, plural = `${singular}s`) {
  return `${count} ${count === 1 ? singular : plural}`
}

function formatRelativeTime(value: number) {
  const diffMinutes = Math.max(0, Math.floor((Date.now() - value) / 60000))

  if (diffMinutes < 1) {
    return "now"
  }

  if (diffMinutes < 60) {
    return `${diffMinutes}m`
  }

  const diffHours = Math.floor(diffMinutes / 60)

  if (diffHours < 24) {
    return `${diffHours}h`
  }

  const diffDays = Math.floor(diffHours / 24)

  if (diffDays < 7) {
    return `${diffDays}d`
  }

  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
  }).format(new Date(value))
}

function getCheckoutStateLabel(summary: ProjectGitCheckoutSummary) {
  if (summary.errorMessage) {
    return "Unavailable"
  }

  if (summary.hasChanges) {
    return "Changed"
  }

  if (summary.ahead > 0 || summary.behind > 0) {
    return "Diverged"
  }

  return "Clean"
}

function getCheckoutDetail(summary: ProjectGitCheckoutSummary) {
  if (summary.errorMessage) {
    return summary.errorMessage
  }

  const details: string[] = [summary.branch ? summary.branch : "Detached HEAD"]

  if (summary.head) {
    details.push(summary.head)
  }

  if (summary.changedCount > 0) {
    details.push(formatCountLabel(summary.changedCount, "changed file"))
  }

  if (summary.untrackedCount > 0) {
    details.push(formatCountLabel(summary.untrackedCount, "untracked file"))
  }

  if (summary.ahead > 0) {
    details.push(`ahead ${summary.ahead}`)
  }

  if (summary.behind > 0) {
    details.push(`behind ${summary.behind}`)
  }

  return details.join(" · ")
}

function getProjectSessions(input: {
  projectPath: string;
  sessions: readonly DaemonSession[];
  sessionWorktrees: readonly ({ worktree: ProjectSessionWorktree | null } | null)[];
}) {
  return input.sessions.map(
    (session, index): SessionWithWorktree => ({
      session,
      worktree: input.sessionWorktrees[index]?.worktree ?? null,
    }),
  ).filter((item) => {
    if (
      findNearestProjectPath(
        [
          { name: "", path: input.projectPath },
        ],
        item.session.cwd,
      )
    ) {
      return true
    }

    return item.worktree?.repoRoot === input.projectPath
  })
}

function getRelatedInboxItems(
  items: readonly InboxItem[],
  sessions: readonly SessionWithWorktree[],
) {
  const projectSessionIds = new Set(sessions.map((item) => item.session.id))

  return items.filter((item) => projectSessionIds.has(item.entityId as DaemonSession["id"]))
}

/** Renders one project-backed closable workbench tab. */
export component ProjectPage(props: { projectName?: string; projectPath: string }) {
  const projectRegistry = useProjectRegistry()
  const workbenchTabSet = useWorkbenchTabSet()

  const project = projectRegistry.projectsByPath[props.projectPath] ?? null

  if (!project) {
    <div class={styles.page}>
      <section class={styles.panel}>
        <h1 class={styles.title}>"Project unavailable"</h1>
        <p class={styles.body}>
          "The project record for this tab is no longer in the machine-wide project registry."
        </p>
      </section>
    </div>

    return
  }

  try {
    const [{ sessions }, { items }, gitStatus] = useQueries(
      (query) => [
        query(goddardSdk.session.list, {
          params: [{ limit: PROJECT_DASHBOARD_SESSION_LIMIT }],
        }),
        query(goddardSdk.inbox.list, {
          params: [
            { statuses: ["unread", "read"], limit: PROJECT_DASHBOARD_INBOX_LIMIT },
          ],
        }),
        query(getProjectGitStatus, {
          params: [project.path],
        }),
      ],
    )
    const sessionWorktrees = useQueries(
      (query) => sessions.map(
        (session) => query(goddardSdk.session.worktree.get, {
          params: [{ id: session.id }],
        }),
      ),
    )
    const projectSessions = getProjectSessions({
      projectPath: project.path,
      sessions,
      sessionWorktrees,
    })
    const relatedInboxItems = getRelatedInboxItems(items, projectSessions)
    const recentSessions = projectSessions.slice(0, 6).map(
      (item) => ({
        session: item.session,
        title: getSessionDisplayTitle(item.session),
      }),
    )

    function openSession(session: DaemonSession) {
      evictSessionHistory(session.id)
      workbenchTabSet.openOrFocusTab({
        kind: "sessionChat",
        props: {
          relatedFilesystemPath: session.cwd,
          sessionId: session.id,
          sessionTitle: getSessionDisplayTitle(session),
        },
      })
    }

    function openSessionChanges(session: DaemonSession) {
      workbenchTabSet.openOrFocusTab({
        kind: "sessionChanges",
        props: {
          repositoryLabel: getSessionRepositoryLabel(session),
          sessionId: session.id,
          sessionTitle: getSessionDisplayTitle(session),
        },
      })
    }

    <div class={styles.page}>
      <header class={styles.header}>
        <div>
          <span class={styles.eyebrow}>"Project dashboard"</span>
          <h1 class={styles.title}>
            {project.name}
          </h1>
          <p class={styles.path}>
            {project.path}
          </p>
        </div>
      </header>

      <div class={styles.grid}>
        <section class={styles.panel}>
          <SectionHeading icon={Inbox} title="Related inbox" count={relatedInboxItems.length} />

          if (relatedInboxItems.length === 0) {
            <EmptyState text="No unread or read inbox messages are linked to this project." />
          } else {
            <div class={styles.list}>
              for (const item of relatedInboxItems) {
                <article class={styles.inboxRow} key={item.id}>
                  <div class={styles.rowMain}>
                    <h2 class={styles.rowTitle}>
                      {getInboxItemPrimaryText(item)}
                    </h2>
                    <p class={styles.rowDescription}>
                      {getInboxItemSecondaryText(item)}
                    </p>
                  </div>
                  <span class={styles.statusBadge} data-status={item.status}>
                    {item.status}
                  </span>
                </article>
              }
            </div>
          }
        </section>

        <section class={styles.panel}>
          <SectionHeading
            icon={GitBranch}
            title="Git checkouts"
            count={gitStatus.worktrees.length + 1}
          />
          <div class={styles.list}>
            <CheckoutRow label="Primary checkout" summary={gitStatus.primary} />

            for (const worktree of gitStatus.worktrees) {
              <CheckoutRow key={worktree.path} label="Worktree" summary={worktree} showArchive />
            }
          </div>
        </section>

        <section class={cx(styles.panel, styles.widePanel)}>
          <SectionHeading
            icon={MessageSquare}
            title="Recent sessions"
            count={recentSessions.length}
          />

          if (recentSessions.length === 0) {
            <EmptyState text="No recent sessions are linked to this project." />
          } else {
            <div class={styles.sessionList}>
              for (const item of recentSessions) {
                <article class={styles.sessionRow} key={item.session.id}>
                  <button
                    class={styles.sessionButton}
                    type="button"
                    onClick={() => {
                      openSession(item.session)
                    }}
                  >
                    <span class={styles.statusDot} data-status={item.session.status} />
                    <span class={styles.rowMain}>
                      <span class={styles.rowTitle}>
                        {item.title}
                      </span>
                      <span class={styles.rowDescription}>
                        {getSessionRepositoryLabel(item.session)}
                      </span>
                    </span>
                    <time
                      class={styles.updated}
                      dateTime={new Date(item.session.updatedAt).toISOString()}
                      title={new Intl.DateTimeFormat(undefined, {
                        dateStyle: "medium",
                        timeStyle: "short",
                      }).format(new Date(item.session.updatedAt))}
                    >
                      {formatRelativeTime(item.session.updatedAt)}
                    </time>
                  </button>
                  <button
                    aria-label={`View changes for ${item.title}`}
                    class={styles.iconButton}
                    type="button"
                    onClick={() => {
                      openSessionChanges(item.session)
                    }}
                  >
                    <FileDiff size={15} strokeWidth={2.1} />
                  </button>
                </article>
              }
            </div>
          }
        </section>
      </div>
    </div>
  } pending {
    <div class={styles.page}>
      <section class={styles.panel}>
        <h1 class={styles.title}>
          {project.name}
        </h1>
        <p class={styles.body}>"Loading project dashboard."</p>
      </section>
    </div>
  } catch (error, retry) {
    <div class={styles.page}>
      <section class={styles.panel}>
        <h1 class={styles.title}>"Couldn't load project dashboard"</h1>
        <p class={styles.body}>
          {error instanceof Error ? error.message : "The project dashboard data request failed."}
        </p>
        <button class={styles.secondaryButton} type="button" onClick={retry}>"Retry"</button>
      </section>
    </div>
  }
}

component SectionHeading(props: { icon: typeof CircleDot; title: string; count: number }) {
  const Icon = props.icon

  <header class={styles.sectionHeader}>
    <div class={styles.sectionTitleRow}>
      <Icon size={16} strokeWidth={2.2} />
      <h2 class={styles.sectionTitle}>
        {props.title}
      </h2>
    </div>
    <span class={styles.countBadge}>
      {props.count}
    </span>
  </header>
}

component EmptyState(props: { text: string }) {
  <p class={styles.empty}>
    {props.text}
  </p>
}

component CheckoutRow(props: {
  label: string;
  summary: ProjectGitCheckoutSummary;
  showArchive?: boolean;
}) {
  <article class={styles.checkoutRow}>
    <div class={styles.rowMain}>
      <div class={styles.checkoutHeader}>
        <h3 class={styles.rowTitle}>
          {props.label}
        </h3>
        <span
          class={styles.statusBadge}
          data-status={getCheckoutStateLabel(props.summary).toLowerCase()}
        >
          {getCheckoutStateLabel(props.summary)}
        </span>
      </div>
      <p class={styles.rowDescription}>
        {getCheckoutDetail(props.summary)}
      </p>
      <p class={styles.path}>
        {props.summary.path}
      </p>
    </div>

    if (props.showArchive) {
      <button
        aria-label={`Archive ${props.summary.path}`}
        class={styles.iconButton}
        type="button"
        onClick={() => {}}
      >
        <Archive size={15} strokeWidth={2.1} />
      </button>
    }
  </article>
}

export default ProjectPage
