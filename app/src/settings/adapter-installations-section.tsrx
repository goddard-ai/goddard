import { t } from "@lingui/core/macro"
import type { AgentCatalogEntry, AgentInstallationState } from "@goddard-ai/sdk"
import { Download, GitBranch, Globe, Search, Trash2 } from "lucide-react"
import * as fuzzysort from "fuzzysort2"
import { useState } from "preact/hooks"

import { useErrorBoundaryReset } from "~/lib/error-boundary-reset.ts"
import { ExternalLink } from "~/lib/external-link.tsrx"
import { queryClient, useQuery } from "~/lib/query.ts"
import { isEmptyQuery } from "~/lib/search-query.ts"
import { goddardSdk } from "~/sdk.ts"
import { AcpAgentIcon } from "~/sessions/acp-agent-icon.tsrx"
import {
  SettingsFieldset,
  SettingsHelperText,
  SettingsLegend,
  SettingsSection,
  SettingsSectionDescription,
  SettingsSectionHeader,
  SettingsSectionTitle,
} from "./styled.tsrx"
import styles from "./adapter-installations-section.style.ts"

const availableManagedAgentMinimumFuzzyScore = 0.12

export const adapterInstallationsSettingsSectionMetadata = {
  id: "settings-managed-agent-installations",
  get description() {
    return t`Choose which agent harnesses are available when launching sessions.`
  },
  get title() {
    return t`Agent harnesses`
  },
}

type SearchableAgentCatalogEntry = {
  managedAgent: AgentCatalogEntry;
  preparedDescription: fuzzysort.PreparedTarget;
  preparedId: fuzzysort.PreparedTarget;
  preparedName: fuzzysort.PreparedTarget;
  preparedVersion: fuzzysort.PreparedTarget;
}

function getInstallationDetail(installation: AgentInstallationState) {
  if (installation.method === "config") {
    return t`Configured`
  }

  if (installation.installed) {
    return t`Installed`
  }

  if (!installation.installable) {
    return t`Unavailable`
  }

  return t`Not installed`
}

function isGithubRepository(repository: string | null | undefined) {
  if (!repository) {
    return false
  }

  try {
    const url = new URL(repository)
    return url.hostname.toLowerCase().replace(/^www\./, "") === "github.com"
  } catch {
    return false
  }
}

function prepareManagedAgentSearchItems(agents: readonly AgentCatalogEntry[]) {
  return agents.map(
    (managedAgent): SearchableAgentCatalogEntry => ({
      managedAgent,
      preparedDescription: fuzzysort.prepare(managedAgent.description),
      preparedId: fuzzysort.prepare(managedAgent.id),
      preparedName: fuzzysort.prepare(managedAgent.name),
      preparedVersion: fuzzysort.prepare(managedAgent.version),
    }),
  )
}

function filterPreparedAvailableManagedAgents(
  agents: readonly SearchableAgentCatalogEntry[],
  searchQuery: string,
) {
  if (isEmptyQuery(searchQuery)) {
    return agents.map((entry) => entry.managedAgent)
  }

  return fuzzysort.searchFields(
    searchQuery,
    agents,
    [
      { key: "name", extract: (entry) => entry.preparedName },
      { key: "id", extract: (entry) => entry.preparedId },
      { key: "description", extract: (entry) => entry.preparedDescription },
      { key: "version", extract: (entry) => entry.preparedVersion },
    ],
    { threshold: availableManagedAgentMinimumFuzzyScore },
  ).items.map((item) => item.value.managedAgent)
}

function ManagedAgentInstallationRow(props: {
  managedAgent: AgentCatalogEntry;
  installation: AgentInstallationState;
  pending: boolean;
  onInstall: (agentId: string) => Promise<void>;
  onUninstall: (agentId: string) => Promise<void>;
}) @{
  const actionDisabled =
    props.pending || props.installation.method === "config" || !props.installation.installable
  const actionLabel =
    props.installation.method === "config"
      ? t`Configured`
      : props.installation.installed
        ? t`Remove`
        : t`Install`
  const ActionIcon = props.installation.installed ? Trash2 : Download

  <li class={styles.adapterItem}>
    <span class={styles.adapterBody}>
      <span class={styles.adapterTitleRow}>
        <AcpAgentIcon class={styles.adapterIcon} icon={props.managedAgent.icon ?? null} />
        <span class={styles.adapterName}>{props.managedAgent.name}</span>
      </span>
      <span class={styles.adapterMeta}>
        {props.managedAgent.version}
        {" · "}
        {getInstallationDetail(props.installation)}
        {props.managedAgent.unofficial ? ` · ${t`Unofficial`}` : ""}
      </span>
      <span class={styles.adapterDescription}>{props.managedAgent.description}</span>

      @if (props.managedAgent.website || props.managedAgent.repository) {
        <span class={styles.adapterLinks}>
          @if (props.managedAgent.website) {
            <ExternalLink class={styles.adapterLink} href={props.managedAgent.website}>
              <Globe aria-hidden={true} size={14} strokeWidth={2.1} />
              {t`Website`}
            </ExternalLink>
          }

          @if (props.managedAgent.repository) {
            <ExternalLink class={styles.adapterLink} href={props.managedAgent.repository}>
              @if (isGithubRepository(props.managedAgent.repository)) {
                <svg aria-hidden={true} class={styles.githubIcon} viewBox="0 0 16 16">
                  <path
                    fill="currentColor"
                    d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82A7.6 7.6 0 0 1 8 3.86c.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8Z"
                  />
                </svg>
              } @else {
                <GitBranch aria-hidden={true} size={14} strokeWidth={2.1} />
              }

              {t`Repository`}
            </ExternalLink>
          }
        </span>
      }
    </span>
    <button
      class={styles.actionButton}
      disabled={actionDisabled}
      type="button"
      onClick={async () => {
        if (props.installation.installed) {
          await props.onUninstall(props.managedAgent.id)
          return
        }

        await props.onInstall(props.managedAgent.id)
      }}
    >
      <ActionIcon aria-hidden={true} size={15} strokeWidth={2.2} />
      {props.pending ? t`Working` : actionLabel}
    </button>
  </li>
}

function ManagedAgentInstallationList() @{
  const managedAgentCatalog = useQuery(goddardSdk.agent.list, {
    params: [{ includeUninstalled: true }],
  })
  const [pendingManagedAgentId, setPendingManagedAgentId] = useState<string | null>(null)
  const [availableAgentSearchQuery, setAvailableAgentSearchQuery] = useState("")
  const installationsByManagedAgentId = new Map(managedAgentCatalog.installations.map(
    (installation) => [installation.agentId, installation],
  ))
  const installedManagedAgents = managedAgentCatalog.agents.filter((managedAgent) => {
    return installationsByManagedAgentId.get(managedAgent.id)?.installed
  })
  const availableManagedAgents = managedAgentCatalog.agents.filter((managedAgent) => {
    const installation = installationsByManagedAgentId.get(managedAgent.id)
    return installation && !installation.installed
  })
  const visibleAvailableManagedAgents = filterPreparedAvailableManagedAgents(
    prepareManagedAgentSearchItems(availableManagedAgents),
    availableAgentSearchQuery,
  )

  async function installManagedAgent(agentId: string) {
    setPendingManagedAgentId(agentId)
    try {
      await goddardSdk.agent.install({ agentId })
      queryClient.invalidate(goddardSdk.agent.list)
    } finally {
      setPendingManagedAgentId(null)
    }
  }

  async function uninstallManagedAgent(agentId: string) {
    setPendingManagedAgentId(agentId)
    try {
      await goddardSdk.agent.uninstall({ agentId })
      queryClient.invalidate(goddardSdk.agent.list)
    } finally {
      setPendingManagedAgentId(null)
    }
  }

  @if (managedAgentCatalog.agents.length === 0) {
    <p class={styles.emptyText}>
      {t`No agent harnesses are available.`}
    </p>
  } @else {
    <div class={styles.agentGroups}>
      <section class={styles.agentGroup}>
        @if (installedManagedAgents.length === 0) {
          <p class={styles.emptyText}>
            {t`No installed agents.`}
          </p>
        } @else {
          <ul class={styles.adapterList}>
            @for (const managedAgent of installedManagedAgents) {
              <ManagedAgentInstallationRow
                key={managedAgent.id}
                {managedAgent}
                installation={installationsByManagedAgentId.get(managedAgent.id)!}
                pending={pendingManagedAgentId === managedAgent.id}
                onInstall={installManagedAgent}
                onUninstall={uninstallManagedAgent}
              />
            }
          </ul>
        }
      </section>
      <section class={styles.agentGroup}>
        <h3 class={styles.agentGroupTitle}>
          {t`Available agents`}
        </h3>

        @if (availableManagedAgents.length > 0) {
          <label class={styles.searchField}>
            <span class={styles.srOnly}>
              {t`Filter available agents`}
            </span>
            <Search aria-hidden={true} class={styles.searchIcon} size={15} strokeWidth={2.1} />
            <input
              class={styles.searchInput}
              placeholder={t`Filter available agents`}
              type="search"
              value={availableAgentSearchQuery}
              onInput={(event) => {
                setAvailableAgentSearchQuery(event.currentTarget.value)
              }}
            />
          </label>
        }
        @if (availableManagedAgents.length === 0) {
          <p class={styles.emptyText}>
            {t`No available agents.`}
          </p>
        } @else if (visibleAvailableManagedAgents.length === 0) {
          <p class={styles.emptyText}>
            {t`No available agents match this search.`}
          </p>
        } @else {
          <ul class={styles.adapterList}>
            @for (const managedAgent of visibleAvailableManagedAgents) {
              <ManagedAgentInstallationRow
                key={managedAgent.id}
                {managedAgent}
                installation={installationsByManagedAgentId.get(managedAgent.id)!}
                pending={pendingManagedAgentId === managedAgent.id}
                onInstall={installManagedAgent}
                onUninstall={uninstallManagedAgent}
              />
            }
          </ul>
        }
      </section>
    </div>
  }
}

export function AdapterInstallationsSettingsSection() @{
  <SettingsSection
    id={adapterInstallationsSettingsSectionMetadata.id}
    aria-labelledby="settings-managed-agent-installations-heading"
  >
    <SettingsSectionHeader>
      <SettingsSectionTitle
        id="settings-managed-agent-installations-heading"
      >{adapterInstallationsSettingsSectionMetadata.title}</SettingsSectionTitle>
      <SettingsSectionDescription>{adapterInstallationsSettingsSectionMetadata.description}</SettingsSectionDescription>
    </SettingsSectionHeader>
    <SettingsFieldset>
      <SettingsLegend>
        {t`Installed agents`}
      </SettingsLegend>
      <SettingsHelperText>
        {t`Launch selectors only show installed agents or agents declared in configuration.`}
      </SettingsHelperText>

      @try {
        <ManagedAgentInstallationList />
      } @pending {
        <p class={styles.emptyText}>
          {t`Loading agent harnesses.`}
        </p>
      } @catch (error, retry) {
        useErrorBoundaryReset(retry)

        <>
          <p class={styles.emptyText}>
            {error instanceof Error ? error.message : t`Failed to load agent harnesses.`}
          </p>
          <button class={styles.actionButton} type="button" onClick={retry}>
            {t`Retry`}
          </button>
        </>
      }
    </SettingsFieldset>
  </SettingsSection>
}
