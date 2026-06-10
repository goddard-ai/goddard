import type { AdapterCatalogEntry, AdapterInstallationState } from "@goddard-ai/sdk"
import { Download, Trash2 } from "lucide-react"
import { useState } from "preact/hooks"

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
  description: "Choose which ACP adapters are available in launch selectors.",
  id: "settings-adapter-installations",
  title: "ACP adapters",
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
    <p class={styles.emptyText}>No ACP adapters are available.</p>
  } @else {
    <ul class={styles.adapterList}>
      @for (const adapter of adapterCatalog.adapters) {
        @if (installationsByAdapterId.get(adapter.id)) {
          <AdapterInstallationRow
            key={adapter.id}
            {adapter}
            installation={installationsByAdapterId.get(adapter.id)!}
            pending={pendingAdapterId === adapter.id}
            onInstall={installAdapter}
            onUninstall={uninstallAdapter}
          />
        }
      }
    </ul>
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
      <SettingsLegend>Installed adapters</SettingsLegend>
      <SettingsHelperText>
        Launch selectors only show adapters installed here or declared in configuration.
      </SettingsHelperText>

      @try {
        <AdapterInstallationList />
      } @pending {
        <p class={styles.emptyText}>Loading ACP adapters.</p>
      } @catch (error, retry) {
        <>
          <p class={styles.emptyText}>
            {error instanceof Error ? error.message : "Failed to load ACP adapters."}
          </p>
          <button class={styles.actionButton} type="button" onClick={retry}>Retry</button>
        </>
      }
    </SettingsFieldset>
  </SettingsSection>
}
