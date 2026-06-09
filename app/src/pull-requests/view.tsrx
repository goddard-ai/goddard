import type { PullRequestId } from "@goddard-ai/pull-request/schema"
import { ExternalLink, GitPullRequest } from "lucide-react"
import { useState } from "preact/hooks"

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
      description="Loading the daemon-managed pull request record."
      title="Loading pull request"
    />
  </main>
}

function PullRequestView(props: {
  relatedFilesystemPath: string | null;
  pullRequestId: PullRequestId;
  pullRequestTitle?: string;
}) @{
  const [, setRetryVersion] = useState(0)

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
          <p class={styles.description}>Daemon-managed pull request linked from the inbox.</p>
        </div>
        <a class={styles.externalLink} href={url} rel="noreferrer" target="_blank">
          <ExternalLink size={15} strokeWidth={2.1} />
          Open on GitHub
        </a>
      </header>
      <section class={styles.detailGrid} aria-label="Pull request details">
        <InfoCard label="Host" value={pullRequest.host} />
        <InfoCard label="Owner" value={pullRequest.owner} />
        <InfoCard label="Repository" value={pullRequest.repo} />
        <InfoCard label="Pull request" value={`#${pullRequest.prNumber}`} />
        <InfoCard label="Workspace" value={pullRequest.cwd} />
      </section>
    </main>
  } @pending {
    <PullRequestLoadingState />
  } @catch (_error) {
    <main class={styles.root}>
      <PullRequestStateMessage
        actionLabel="Try again"
        description="The daemon-backed pull request record could not be loaded."
        onAction={() => {
          retryPullRequestLoad(props.pullRequestId)
          setRetryVersion((version) => version + 1)
        }}
        title="Couldn't load pull request"
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

export default PullRequestView
