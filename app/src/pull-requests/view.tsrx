import type { PullRequestId } from "@goddard-ai/pull-request/schema"
import { ExternalLink as ExternalLinkIcon, GitPullRequest } from "lucide-react"

import { text } from "~/language/text.ts"
import { useErrorBoundaryReset } from "~/lib/error-boundary-reset.ts"
import { ExternalLink } from "~/lib/external-link.tsrx"
import { queryClient, useQuery } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"
import { getPullRequestDisplayTitle } from "./display.ts"
import { getPullRequestUrl } from "./links.ts"
import styles from "./view.style.ts"

function retryPullRequestLoad(pullRequestId: PullRequestId) {
  queryClient.invalidate(goddardSdk.pr.get, [{ id: pullRequestId }])
}

function PullRequestStateMessage(props: {
  actionLabel?: string;
  description: string;
  onAction?: () => void;
  title: string;
}) @{
  <div class={styles.stateMessage}>
    <h2 class={styles.stateTitle}>{props.title}</h2>
    <p class={styles.stateDescription}>{props.description}</p>

    {props.actionLabel && props.onAction
      ? <button
          class={styles.stateAction}
          type="button"
          onClick={props.onAction}
        >{props.actionLabel}</button>
      : null}
  </div>
}

function PullRequestLoadingState() @{
  <main class={styles.root}>
    <PullRequestStateMessage
      description={text.loadingTheDaemonManagedPullRequestRecord}
      title={text.loadingPullRequest}
    />
  </main>
}

export default function PullRequestView(props: {
  relatedFilesystemPath: string | null;
  pullRequestId: PullRequestId;
  pullRequestTitle?: string;
}) @{
  @try {
    const { pullRequest } = useQuery(goddardSdk.pr.get, { params: [{ id: props.pullRequestId }] })
    const title = getPullRequestDisplayTitle(pullRequest)
    const url = getPullRequestUrl(pullRequest)

    <main class={styles.root}>
      <header class={styles.header}>
        <div class={styles.headerIdentity}>
          <p class={styles.repository}>
            {pullRequest.owner}
            {"/"}
            {pullRequest.repo}
          </p>
          <h1 class={styles.title}>
            <GitPullRequest size={18} strokeWidth={2.1} />
            {title}
          </h1>
          <p class={styles.description}>{text.daemonManagedPullRequestLinkedFromTheInbox}</p>
        </div>
        <ExternalLink class={styles.externalLink} href={url}>
          <ExternalLinkIcon size={15} strokeWidth={2.1} />
          {text.openOnGitHub}
        </ExternalLink>
      </header>
      <section class={styles.detailGrid} aria-label={text.pullRequestDetails}>
        <InfoCard label={text.host} value={pullRequest.host} />
        <InfoCard label={text.owner} value={pullRequest.owner} />
        <InfoCard label={text.repository} value={pullRequest.repo} />
        <InfoCard label={text.pullRequest} value={`#${pullRequest.prNumber}`} />
        <InfoCard label={text.workspace} value={pullRequest.cwd} />
      </section>
    </main>
  } @pending {
    <PullRequestLoadingState />
  } @catch (_error, retry) {
    const resetPullRequestBoundary = () => {
      retryPullRequestLoad(props.pullRequestId)
      retry()
    }
    useErrorBoundaryReset(resetPullRequestBoundary)

    <main class={styles.root}>
      <PullRequestStateMessage
        actionLabel={text.tryAgain}
        description={text.theDaemonBackedPullRequestRecordCouldNotBeLoaded}
        onAction={resetPullRequestBoundary}
        title={text.couldntLoadPullRequest}
      />
    </main>
  }
}

function InfoCard(props: {
  label: string;
  value: string;
}) @{
  <dl class={styles.infoCard}>
    <dt class={styles.infoLabel}>{props.label}</dt>
    <dd class={styles.infoValue}>{props.value}</dd>
  </dl>
}
