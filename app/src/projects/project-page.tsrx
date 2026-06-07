import type { InboxItem } from "@goddard-ai/inbox/schema"
import type { DaemonSession } from "@goddard-ai/sdk"
import type { ProjectGitCheckoutSummary } from "~/shared/desktop-rpc.ts"
import { useModel } from "@preact/signals"
import {
  AlertCircle,
  Archive,
  ArrowDown,
  ArrowUp,
  ArrowUpDown,
  CircleDot,
  FileDiff,
  GitBranch,
  GitFork,
  Inbox,
  Laptop,
  MessageSquare,
} from "lucide-react"
import { useEffect } from "preact/hooks"

import { useProjectRegistry, useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { AppCommand, useAppCommand } from "~/commands/app-command.ts"
import { getProjectGitStatus } from "~/desktop-host.ts"
import { getInboxItemPrimaryText, getInboxItemSecondaryText } from "~/inbox/text.ts"
import { appToaster } from "~/lib/good-toaster.tsrx"
import { GoodTooltip } from "~/lib/good-tooltip.tsrx"
import { useQueries } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"
import { getSessionDisplayTitle, getSessionRepositoryLabel } from "~/sessions/display.ts"
import { SessionLaunchForm } from "~/sessions/launch-form.tsrx"
import { SessionLaunchFormState } from "~/sessions/launch-form-state.ts"
import { isKeptSessionLaunchLease } from "~/sessions/launch-lease-keeper.tsrx"
import { recordSessionLaunchAgentUse } from "~/sessions/launch-preferences.ts"
import { evictSessionHistory } from "~/sessions/mutations.ts"
import { createSession, releaseSessionLaunchLease } from "~/sessions/mutations.ts"
import { findNearestProjectPath } from "./project-context.ts"
import type { ProjectRecord } from "./project-registry.ts"
import styles from "./project-page.style.ts"

const PROJECT_DASHBOARD_SESSION_LIMIT = 50
const PROJECT_DASHBOARD_INBOX_LIMIT = 50

type SessionWithWorktree = {
  session: DaemonSession;
  worktree: ProjectSessionWorktree | null;
}

type ProjectSessionWorktree = {
  repoRoot: string;
  worktreeDir: string;
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

function getCheckoutStatePresentation(summary: ProjectGitCheckoutSummary) {
  if (summary.errorMessage) {
    return {
      icon: AlertCircle,
      label: "Git status unavailable",
      tone: "danger",
    }
  }

  if (summary.ahead > 0 && summary.behind > 0) {
    return {
      icon: ArrowUpDown,
      label: `Ahead ${summary.ahead}, behind ${summary.behind}`,
      tone: "attention",
    }
  }

  if (summary.ahead > 0) {
    return {
      icon: ArrowUp,
      label: `Ahead ${summary.ahead}`,
      tone: "attention",
    }
  }

  if (summary.behind > 0) {
    return {
      icon: ArrowDown,
      label: `Behind ${summary.behind}`,
      tone: "attention",
    }
  }

  if (summary.hasChanges) {
    return {
      icon: FileDiff,
      label: "Working tree has changes",
      tone: "attention",
    }
  }

  return null
}

function getCheckoutDetail(summary: ProjectGitCheckoutSummary) {
  if (summary.errorMessage) {
    return summary.errorMessage
  }

  const details: string[] = []

  if (summary.changedCount > 0) {
    details.push(formatCountLabel(summary.changedCount, "changed file"))
  }

  if (summary.untrackedCount > 0) {
    details.push(formatCountLabel(summary.untrackedCount, "untracked file"))
  }

  if (details.length === 0) {
    return summary.path
  }

  return `${details.join(" • ")} • ${summary.path}`
}

function getCheckoutMainLabel(summary: ProjectGitCheckoutSummary) {
  if (summary.errorMessage) {
    return "Git status unavailable"
  }

  const details: string[] = [summary.branch ? summary.branch : "Detached HEAD"]

  if (summary.head) {
    details.push(summary.head)
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

    <ProjectLaunchSection {project} />

    try {
      <ProjectDashboardSections {project} />
    } pending {
      <section class={styles.panel}>
        <p class={styles.body}>"Loading project dashboard."</p>
      </section>
    } catch (error, retry) {
      <section class={styles.panel}>
        <h2 class={styles.sectionTitle}>"Couldn't load project dashboard"</h2>
        <p class={styles.body}>
          {error instanceof Error ? error.message : "The project dashboard data request failed."}
        </p>
        <button class={styles.secondaryButton} type="button" onClick={retry}>"Retry"</button>
      </section>
    }
  </div>
}

component ProjectLaunchSection(props: { project: ProjectRecord }) {
  const form = useModel(SessionLaunchFormState)
  const workbenchTabSet = useWorkbenchTabSet()
  const hasAdapterSelector = (form.adapterCatalog.value?.adapters.length ?? 0) > 0
  const hasBranchSelector =
    (form.launchPreview.value?.branches.length ?? 0) > 0 &&
      !(form.draftLocation.value === "local" && form.launchPreview.value?.dirty === true)
  const hasLocationSelector = !!form.launchPreview.value?.repoRoot
  const hasModelSelector = (form.launchModelConfig.value.models?.availableModels.length ?? 0) > 0
  const hasSubpackageSelector = form.draftProjectPath.value !== null
  const hasThinkingLevel = form.thinkingOption.value !== null
  const canSubmit = form.canSubmit.value

  useEffect(() => {
    if (form.draftProjectPath.value !== props.project.path) {
      form.reset(props.project.path)
    }
  }, [form, form.draftProjectPath.value, props.project.path])

  async function launchSession() {
    const sessionInput = form.sessionInput.value

    if (!sessionInput) {
      return
    }

    try {
      const launchLeaseId = form.launchPreview.value?.launchLeaseId
      recordSessionLaunchAgentUse(form.draftAdapterId.value)
      const { session } = await createSession(sessionInput)
      const sessionTitle = getSessionDisplayTitle(session)

      if (!isKeptSessionLaunchLease(launchLeaseId)) {
        void releaseSessionLaunchLease(launchLeaseId).catch((error) => {
          console.error("Failed to release launch lease.", error)
        })
      }

      form.reset(props.project.path)
      workbenchTabSet.openOrFocusTab({
        kind: "sessionChat",
        props: {
          relatedFilesystemPath: props.project.path,
          sessionId: session.id,
          sessionTitle,
        },
      })
      window.setTimeout(() => {
        appToaster.create({
          description: sessionTitle,
          duration: 2800,
          title: "Session launched",
          type: "success",
        })
      }, 0)
    } catch (error) {
      console.error("Failed to create session.", error)
    }
  }

  useAppCommand(AppCommand.sessionInput.openProjectSelector, () => {
    form.setOpenPicker("project")
  }, { active: false })
  useAppCommand(AppCommand.sessionInput.openSubpackageSelector, () => {
    form.setOpenPicker("subpackage")
  }, { active: hasSubpackageSelector })
  useAppCommand(AppCommand.sessionInput.openAdapterSelector, () => {
    form.setOpenPicker("adapter")
  }, { active: hasAdapterSelector })
  useAppCommand(AppCommand.sessionInput.openLocationSelector, () => {
    form.cycleLaunchLocation()
  }, { active: hasLocationSelector })
  useAppCommand(AppCommand.sessionInput.openBranchSelector, () => {
    form.setOpenPicker("branch")
  }, { active: hasBranchSelector })
  useAppCommand(AppCommand.sessionInput.openApprovalPresetSelector, () => {
    form.setOpenPicker("mode")
  }, { active: form.modeOption.value !== null })
  useAppCommand(AppCommand.sessionInput.openModelSelector, () => {
    form.setOpenPicker("model")
  }, { active: hasModelSelector })
  useAppCommand(AppCommand.sessionInput.openThinkingLevelSelector, () => {
    form.setOpenPicker("thinking")
  }, { active: hasThinkingLevel })
  useAppCommand(AppCommand.sessionInput.submit, () => {
    void launchSession()
  }, { active: canSubmit })

  <section class={styles.launchPanel}>
    <SessionLaunchForm
      {form}
      showProjectSelector={false}
      onSubmit={launchSession}
      projects={[props.project]}
    />
  </section>
}

component ProjectDashboardSections(props: { project: ProjectRecord }) {
  const workbenchTabSet = useWorkbenchTabSet()

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
          params: [props.project.path],
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
      projectPath: props.project.path,
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
    const goddardWorktrees = projectSessions.flatMap((item) => {
      const worktree = item.worktree

      if (!worktree) {
        return []
      }

      const summary = gitStatus.worktrees.find(
        (candidate) => candidate.path === worktree.worktreeDir,
      )

      if (!summary) {
        return []
      }

      return [
        {
          session: item.session,
          summary,
        },
      ]
    })

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

    <div class={styles.grid}>
      <div class={styles.column}>
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

      <section class={styles.panel}>
        <SectionHeading
          icon={GitBranch}
          title="Git checkouts"
          count={goddardWorktrees.length + 1}
        />
        <div class={styles.list}>
          <CheckoutRow kind="primary" summary={gitStatus.primary} />

          for (const worktree of goddardWorktrees) {
            <CheckoutRow
              key={worktree.summary.path}
              kind="worktree"
              summary={worktree.summary}
              showArchive
              onViewChanges={() => openSessionChanges(worktree.session)}
            />
          }
        </div>
      </section>
    </div>
  } pending {
    <section class={styles.panel}>
      <p class={styles.body}>"Loading project dashboard."</p>
    </section>
  } catch (error, retry) {
    <section class={styles.panel}>
      <h2 class={styles.sectionTitle}>"Couldn't load project dashboard"</h2>
      <p class={styles.body}>
        {error instanceof Error ? error.message : "The project dashboard data request failed."}
      </p>
      <button class={styles.secondaryButton} type="button" onClick={retry}>"Retry"</button>
    </section>
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
  kind: "primary" | "worktree";
  onViewChanges?: () => void;
  summary: ProjectGitCheckoutSummary;
  showArchive?: boolean;
}) {
  const KindIcon =
    props.kind === "primary" ? Laptop : GitFork
  const kindLabel =
    props.kind === "primary" ? "Primary checkout" : "Worktree"
  const checkoutState = getCheckoutStatePresentation(props.summary)
  const CheckoutStateIcon = checkoutState?.icon

  <article class={styles.checkoutRow}>
    <GoodTooltip content={kindLabel}>
      <span class={styles.checkoutIcon} aria-label={kindLabel}>
        <KindIcon aria-hidden={true} size={16} strokeWidth={2.2} />
      </span>
    </GoodTooltip>
    <div class={styles.rowMain}>
      <div class={styles.checkoutHeader}>
        <h3 class={styles.rowTitle}>
          {getCheckoutMainLabel(props.summary)}
        </h3>

        if (checkoutState && CheckoutStateIcon) {
          <GoodTooltip content={checkoutState.label}>
            <span
              class={styles.checkoutStateIcon}
              data-tone={checkoutState.tone}
              aria-label={checkoutState.label}
            >
              <CheckoutStateIcon aria-hidden={true} size={14} strokeWidth={2.2} />
            </span>
          </GoodTooltip>
        }
      </div>
      <p class={styles.rowDescription}>
        {getCheckoutDetail(props.summary)}
      </p>
    </div>

    if (props.kind === "worktree") {
      <div class={styles.checkoutActions} data-checkout-actions="true">
        <GoodTooltip content="View changes">
          <button
            aria-label={`View changes for ${props.summary.path}`}
            class={styles.iconButton}
            type="button"
            onClick={() => {
              props.onViewChanges?.()
            }}
          >
            <FileDiff size={15} strokeWidth={2.1} />
          </button>
        </GoodTooltip>

        if (props.showArchive) {
          <GoodTooltip content="Archive">
            <button
              aria-label={`Archive ${props.summary.path}`}
              class={styles.iconButton}
              type="button"
              onClick={() => {}}
            >
              <Archive size={15} strokeWidth={2.1} />
            </button>
          </GoodTooltip>
        }
      </div>
    }
  </article>
}

export default ProjectPage
