import type { AdapterCatalogEntry, AdapterInstallationState } from "@goddard-ai/sdk"
import { Download, GitBranch, Globe, Trash2 } from "lucide-react"
import { useState } from "preact/hooks"

import { ExternalLink } from "~/lib/external-link.tsrx"
import { queryClient, useQuery } from "~/lib/query.ts"
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

export const adapterInstallationsSettingsSectionMetadata = {
  description: "Choose which agent harnesses are available when launching sessions.",
  id: "settings-adapter-installations",
  title: "Agent harnesses",
}

function getInstallationDetail(installation: AdapterInstallationState) {
  if (installation.method === "config") {
    return "Configured"
  }

  if (installation.installed) {
    return "Installed"
  }

  if (!installation.installable) {
    return "Unavailable"
  }

  return "Not installed"
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

function AdapterInstallationRow(props: {
  adapter: AdapterCatalogEntry;
  installation: AdapterInstallationState;
  pending: boolean;
  onInstall: (adapterId: string) => Promise<void>;
  onUninstall: (adapterId: string) => Promise<void>;
}) @{
  const actionDisabled =
    props.pending || props.installation.method === "config" || !props.installation.installable
  const actionLabel =
    props.installation.method === "config"
      ? "Configured"
      : props.installation.installed
        ? "Remove"
        : "Install"
  const ActionIcon = props.installation.installed ? Trash2 : Download

  <li class={styles.adapterItem}>
    <span class={styles.adapterBody}>
      <span class={styles.adapterTitleRow}>
        <AcpAgentIcon class={styles.adapterIcon} icon={props.adapter.icon ?? null} />
        <span class={styles.adapterName}>{props.adapter.name}</span>
      </span>
      <span class={styles.adapterMeta}>
        {props.adapter.version}
        {" · "}
        {getInstallationDetail(props.installation)}
        {props.adapter.unofficial ? " · Unofficial" : ""}
      </span>
      <span class={styles.adapterDescription}>{props.adapter.description}</span>
      @if (props.adapter.website || props.adapter.repository) {
        <span class={styles.adapterLinks}>
          @if (props.adapter.website) {
            <ExternalLink class={styles.adapterLink} href={props.adapter.website}>
              <Globe aria-hidden={true} size={14} strokeWidth={2.1} />
              Website
            </ExternalLink>
          }

          @if (props.adapter.repository) {
            <ExternalLink class={styles.adapterLink} href={props.adapter.repository}>
              @if (isGithubRepository(props.adapter.repository)) {
                <svg aria-hidden={true} class={styles.githubIcon} viewBox="0 0 16 16">
                  <path
                    fill="currentColor"
                    d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82A7.6 7.6 0 0 1 8 3.86c.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8Z"
                  />
                </svg>
              } @else {
                <GitBranch aria-hidden={true} size={14} strokeWidth={2.1} />
              }
              Repository
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
          await props.onUninstall(props.adapter.id)
          return
        }

        await props.onInstall(props.adapter.id)
      }}
    >
      <ActionIcon aria-hidden={true} size={15} strokeWidth={2.2} />
      {props.pending ? "Working" : actionLabel}
    </button>
  </li>
}

function AdapterInstallationList() @{
  const adapterCatalog = useQuery(goddardSdk.adapter.list, {
    params: [{ includeUninstalled: true }],
  })
  const [pendingAdapterId, setPendingAdapterId] = useState<string | null>(null)
  const installationsByAdapterId = new Map(adapterCatalog.installations.map(
    (installation) => [installation.adapterId, installation],
  ))
  const installedAdapters = adapterCatalog.adapters.filter((adapter) => {
    return installationsByAdapterId.get(adapter.id)?.installed
  })
  const availableAdapters = adapterCatalog.adapters.filter((adapter) => {
    const installation = installationsByAdapterId.get(adapter.id)
    return installation && !installation.installed
  })

  async function installAdapter(adapterId: string) {
    setPendingAdapterId(adapterId)
    try {
      await goddardSdk.adapter.install({ adapterId })
      queryClient.invalidate(goddardSdk.adapter.list)
    } finally {
      setPendingAdapterId(null)
    }
  }

  async function uninstallAdapter(adapterId: string) {
    setPendingAdapterId(adapterId)
    try {
      await goddardSdk.adapter.uninstall({ adapterId })
      queryClient.invalidate(goddardSdk.adapter.list)
    } finally {
      setPendingAdapterId(null)
    }
  }

  @if (adapterCatalog.adapters.length === 0) {
    <p class={styles.emptyText}>No agent harnesses are available.</p>
  } @else {
    <div class={styles.agentGroups}>
      <section class={styles.agentGroup}>
        @if (installedAdapters.length === 0) {
          <p class={styles.emptyText}>No installed agents.</p>
        } @else {
          <ul class={styles.adapterList}>
            @for (const adapter of installedAdapters) {
              <AdapterInstallationRow
                key={adapter.id}
                {adapter}
                installation={installationsByAdapterId.get(adapter.id)!}
                pending={pendingAdapterId === adapter.id}
                onInstall={installAdapter}
                onUninstall={uninstallAdapter}
              />
            }
          </ul>
        }
      </section>
      <section class={styles.agentGroup}>
        <h3 class={styles.agentGroupTitle}>Available agents</h3>
        @if (availableAdapters.length === 0) {
          <p class={styles.emptyText}>No available agents.</p>
        } @else {
          <ul class={styles.adapterList}>
            @for (const adapter of availableAdapters) {
              <AdapterInstallationRow
                key={adapter.id}
                {adapter}
                installation={installationsByAdapterId.get(adapter.id)!}
                pending={pendingAdapterId === adapter.id}
                onInstall={installAdapter}
                onUninstall={uninstallAdapter}
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
    aria-labelledby="settings-adapter-installations-heading"
  >
    <SettingsSectionHeader>
      <SettingsSectionTitle
        id="settings-adapter-installations-heading"
      >{adapterInstallationsSettingsSectionMetadata.title}</SettingsSectionTitle>
      <SettingsSectionDescription>{adapterInstallationsSettingsSectionMetadata.description}</SettingsSectionDescription>
    </SettingsSectionHeader>
    <SettingsFieldset>
      <SettingsLegend>Installed agents</SettingsLegend>
      <SettingsHelperText>
        Launch selectors only show installed agents or agents declared in configuration.
      </SettingsHelperText>

      @try {
        <AdapterInstallationList />
      } @pending {
        <p class={styles.emptyText}>Loading agent harnesses.</p>
      } @catch (error, retry) {
        <>
          <p class={styles.emptyText}>
            {error instanceof Error ? error.message : "Failed to load agent harnesses."}
          </p>
          <button class={styles.actionButton} type="button" onClick={retry}>Retry</button>
        </>
      }
    </SettingsFieldset>
  </SettingsSection>
}
