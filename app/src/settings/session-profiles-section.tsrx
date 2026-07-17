import { t } from "@lingui/core/macro"
import {
  deriveSessionLaunchModelConfig,
  deriveSessionProfileConfig,
  type SessionLaunchPreviewResponse,
  type SessionProfile,
  type SessionProfileId,
} from "@goddard-ai/sdk"
import { useSignal } from "@preact/signals"
import { Save, Trash2 } from "lucide-react"
import { useEffect, useState } from "preact/hooks"

import { useProjectContext, useProjectRegistry } from "~/app-state-context.tsrx"
import { useErrorBoundaryReset } from "~/lib/error-boundary-reset.ts"
import { appToaster } from "~/lib/good-toaster.tsrx"
import { useQuery } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"
import { formatSessionRequestError } from "~/session-chat/request-error.ts"
import { findSelectConfigOption } from "~/session-input/config-options.ts"
import { SessionInputSelect, type SessionInputSelectItem } from "~/session-input/select-menu.tsrx"
import { SessionConfigOptionSelector } from "~/session-input/session-mode-selector.tsrx"
import {
  resolvePreferredLaunchAgentId,
  resolvePreferredLaunchCwd,
} from "~/sessions/launch-preferences.ts"
import { isKeptSessionLaunchLease } from "~/sessions/launch-lease-keeper.tsrx"
import {
  releaseSessionLaunchLease,
  removeSessionProfile,
  setSessionProfile,
} from "~/sessions/mutations.ts"
import { getSessionProfileLabel, sessionProfileIds } from "~/sessions/session-profiles.ts"
import {
  SettingsFieldset,
  SettingsHelperText,
  SettingsLegend,
  SettingsSection,
  SettingsSectionDescription,
  SettingsSectionHeader,
  SettingsSectionTitle,
} from "./styled.tsrx"
import styles from "./session-profiles-section.style.ts"

export const sessionProfilesSettingsSectionMetadata = {
  id: "settings-session-profiles",
  get description() {
    return t`Save exact model, thinking, and approval selections for each agent harness.`
  },
  get title() {
    return t`Session profiles`
  },
}

type SessionProfileEditorProps = {
  agentId: string;
  configOptions: SessionLaunchPreviewResponse["configOptions"];
  profile: SessionProfile | undefined;
  profileId: SessionProfileId;
}

function SessionProfileEditor(props: SessionProfileEditorProps) @{
  const modelPickerOpen = useSignal(false)
  const thinkingPickerOpen = useSignal(false)
  const modePickerOpen = useSignal(false)
  const draftModelId = useSignal<string | null>(null)
  const draftThinkingValue = useSignal<string | null>(null)
  const draftModeValue = useSignal<string | null>(null)
  const [pending, setPending] = useState(false)
  const [errorMessage, setErrorMessage] = useState<string | null>(null)
  const launchConfig = deriveSessionLaunchModelConfig({ configOptions: props.configOptions })
  const profileConfig = deriveSessionProfileConfig({ configOptions: props.configOptions })
  const resolvedProfile =
    props.profile ? profileConfig.resolveProfile(props.profile) : null
  const thinkingOption = findSelectConfigOption(launchConfig.configOptions, "thought_level")
  const modeOption = findSelectConfigOption(launchConfig.configOptions, "mode")
  const modelItems: SessionInputSelectItem[] =
    launchConfig.models?.availableModels.map(
      (model) => ({
        value: model.modelId,
        label: model.name,
        detail: model.description ?? model.modelId,
        searchText: [model.name, model.description, model.modelId].filter(Boolean).join("\n"),
      }),
    ) ??
      []
  const candidateProfile = profileConfig.createProfile({
    modelId: draftModelId.value,
    thinkingValue: draftThinkingValue.value,
    approvalModeValue: draftModeValue.value,
  })

  useEffect(() => {
    const availableProfile =
      resolvedProfile?.status === "available" ? resolvedProfile : null

    draftModelId.value = availableProfile?.modelId ?? launchConfig.models?.currentModelId ?? null
    draftThinkingValue.value = availableProfile?.thinkingValue ?? thinkingOption?.currentValue ??
      null
    draftModeValue.value = availableProfile?.approvalModeValue ?? modeOption?.currentValue ?? null
    setErrorMessage(null)
  }, [props.agentId, props.profile, props.profileId, props.configOptions])

  async function saveProfile() {
    if (!candidateProfile) {
      return
    }

    setPending(true)
    setErrorMessage(null)
    try {
      await setSessionProfile({
        agentId: props.agentId,
        profileId: props.profileId,
        profile: candidateProfile,
      })
      appToaster.create({
        title: t`${getSessionProfileLabel(props.profileId)} profile saved`,
        type: "success",
      })
    } catch (error) {
      setErrorMessage(formatSessionRequestError(error))
    } finally {
      setPending(false)
    }
  }

  async function clearProfile() {
    setPending(true)
    setErrorMessage(null)
    try {
      await removeSessionProfile({
        agentId: props.agentId,
        profileId: props.profileId,
      })
    } catch (error) {
      setErrorMessage(formatSessionRequestError(error))
    } finally {
      setPending(false)
    }
  }

  <section class={styles.profileRow} aria-labelledby={`session-profile-${props.profileId}-heading`}>
    <div class={styles.profileHeader}>
      <div class={styles.profileIdentity}>
        <h3
          class={styles.profileTitle}
          id={`session-profile-${props.profileId}-heading`}
        >{getSessionProfileLabel(props.profileId)}</h3>
        <span
          class={styles.profileStatus}
          data-tone={resolvedProfile?.status === "unavailable" ? "warning" : undefined}
        >
          {resolvedProfile?.status === "unavailable"
            ? t`Unavailable`
            : props.profile
              ? t`Configured`
              : t`Not configured`}
        </span>
      </div>
      <div class={styles.profileActions}>
        <button
          aria-label={t`Remove ${getSessionProfileLabel(props.profileId)} profile`}
          class={styles.iconButton}
          disabled={pending || !props.profile}
          title={t`Remove profile`}
          type="button"
          onClick={clearProfile}
        >
          <Trash2 aria-hidden={true} size={15} strokeWidth={2.1} />
        </button>
        <button
          class={styles.saveButton}
          disabled={pending || !candidateProfile}
          type="button"
          onClick={saveProfile}
        >
          <Save aria-hidden={true} size={15} strokeWidth={2.1} />
          {pending ? t`Saving` : t`Save`}
        </button>
      </div>
    </div>

    <div class={styles.profileControls}>
      <SessionInputSelect
        disabled={pending || modelItems.length === 0}
        filterable={true}
        items={modelItems}
        label={t`Model`}
        open={modelPickerOpen}
        placeholder={t`Select model`}
        value={draftModelId.value}
        onValueChange={(value) => {
          draftModelId.value = value
        }}
      />
      <SessionConfigOptionSelector
        disabled={pending}
        label={t`Thinking level`}
        option={thinkingOption
          ? {
              ...thinkingOption,
              currentValue: draftThinkingValue.value ?? thinkingOption.currentValue,
            }
          : null}
        open={thinkingPickerOpen}
        placeholder={t`Select thinking level`}
        onValueChange={(value) => {
          draftThinkingValue.value = value
        }}
      />
      <SessionConfigOptionSelector
        disabled={pending}
        label={t`Approval preset`}
        option={modeOption
          ? { ...modeOption, currentValue: draftModeValue.value ?? modeOption.currentValue }
          : null}
        open={modePickerOpen}
        placeholder={t`Select approval preset`}
        onValueChange={(value) => {
          draftModeValue.value = value
        }}
      />
    </div>

    @if (resolvedProfile?.status === "unavailable") {
      <p class={styles.profileMessage}>
        {t`One or more saved options are no longer advertised by this agent harness.`}
      </p>
    }
    @if (errorMessage) {
      <p class={styles.errorMessage} role="alert">{errorMessage}</p>
    }
  </section>
}

function SessionProfilesManager(props: { projectPath: string }) @{
  const subpackages = useQuery(goddardSdk.session.subpackages, {
    params: [{ cwd: props.projectPath }],
  })
  const cwd = resolvePreferredLaunchCwd(props.projectPath, subpackages.subpackages)
  const agentCatalog = useQuery(goddardSdk.agent.list, {
    params: [
      { cwd, includeUninstalled: false },
    ],
  })
  const selectedAgentId = useSignal<string | null>(null)
  const agentItems: SessionInputSelectItem[] = agentCatalog.agents.map(
    (agent) => ({
      value: agent.id,
      label: agent.name,
      detail: agent.version,
      icon: agent.icon,
      searchText: `${agent.id}\n${agent.description ?? ""}`,
    }),
  )
  const agentPickerOpen = useSignal(false)

  useEffect(() => {
    if (
      selectedAgentId.value &&
        agentCatalog.agents.some((agent) => agent.id === selectedAgentId.value)
    ) {
      return
    }

    selectedAgentId.value = resolvePreferredLaunchAgentId(agentCatalog)
  }, [agentCatalog, selectedAgentId])

  const preview = useQuery(selectedAgentId.value ? goddardSdk.session.launchPreview : null, {
    params: selectedAgentId.value
      ? [
          { agent: selectedAgentId.value, cwd },
        ]
      : null,
  })
  const profilesResponse = useQuery(goddardSdk.session.profile.list, {
    params: [{}],
  })

  useEffect(() => {
    const launchLeaseId = preview?.launchLeaseId

    return () => {
      if (!launchLeaseId || isKeptSessionLaunchLease(launchLeaseId)) {
        return
      }

      void releaseSessionLaunchLease(launchLeaseId).catch((error) => {
        console.error("Failed to release the session-profile launch lease.", error)
      })
    }
  }, [preview?.launchLeaseId])

  @if (agentItems.length === 0) {
    <p class={styles.emptyText}>
      {t`No installed agent harnesses are available.`}
    </p>
  } @else {
    <>
      <div class={styles.agentSelector}>
        <SessionInputSelect
          filterable={true}
          items={agentItems}
          label={t`Agent harness`}
          open={agentPickerOpen}
          placeholder={t`Select agent harness`}
          value={selectedAgentId.value}
          onValueChange={(value) => {
            selectedAgentId.value = value
          }}
        />
        <span class={styles.projectContext}>{props.projectPath}</span>
      </div>

      @if (preview && selectedAgentId.value) {
        const agentId = selectedAgentId.value

        <div class={styles.profileList}>
          @for (const profileId of sessionProfileIds) {
            <SessionProfileEditor
              key={`${agentId}:${profileId}`}
              {agentId}
              configOptions={preview.configOptions}
              profile={profilesResponse.profiles[agentId]?.[profileId]}
              {profileId}
            />
          }
        </div>
      }
    </>
  }
}

export function SessionProfilesSettingsSection() @{
  const projectContext = useProjectContext()
  const projectRegistry = useProjectRegistry()
  const projectPath =
    projectContext.activeProjectPath ?? projectRegistry.projectList[0]?.path ?? null

  <SettingsSection
    id={sessionProfilesSettingsSectionMetadata.id}
    aria-labelledby="settings-session-profiles-heading"
  >
    <SettingsSectionHeader>
      <SettingsSectionTitle
        id="settings-session-profiles-heading"
      >{sessionProfilesSettingsSectionMetadata.title}</SettingsSectionTitle>
      <SettingsSectionDescription>{sessionProfilesSettingsSectionMetadata.description}</SettingsSectionDescription>
    </SettingsSectionHeader>
    <SettingsFieldset>
      <SettingsLegend>
        {t`Profile definitions`}
      </SettingsLegend>
      <SettingsHelperText>
        {t`Profiles are global and use options advertised in the current project context.`}
      </SettingsHelperText>

      @if (!projectPath) {
        <p class={styles.emptyText}>
          {t`Add a project to configure session profiles.`}
        </p>
      } @else {
        @try {
          <SessionProfilesManager {projectPath} />
        } @pending {
          <p class={styles.emptyText}>
            {t`Loading session profile options.`}
          </p>
        } @catch (error, retry) {
          useErrorBoundaryReset(retry)

          <>
            <p class={styles.errorMessage} role="alert">{formatSessionRequestError(error)}</p>
            <button class={styles.saveButton} type="button" onClick={retry}>
              {t`Retry`}
            </button>
          </>
        }
      }
    </SettingsFieldset>
  </SettingsSection>
}
