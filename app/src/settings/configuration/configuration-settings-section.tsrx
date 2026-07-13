import { t } from "@lingui/core/macro"
import { isIpcClientErrorForRegistry, type IpcClientErrorForRegistry } from "@goddard-ai/ipc"
import { UserConfigErrorCodes, UserConfigIpcErrors, type UserConfigDocument } from "@goddard-ai/sdk"
import type { ErrorSchema, RJSFSchema } from "@rjsf/utils"
import { RefreshCw } from "lucide-react"
import { useEffect, useState } from "preact/hooks"

import { useErrorBoundaryReset } from "~/lib/error-boundary-reset.ts"
import { useQuery } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"
import {
  SettingsFieldset,
  SettingsHelperText,
  SettingsLegend,
  SettingsSection,
  SettingsSectionDescription,
  SettingsSectionHeader,
  SettingsSectionTitle,
} from "~/settings/styled.tsrx"
import { ConfigurationForm } from "./configuration-form.tsx"
import styles from "./configuration-settings-section.style.ts"
import { deriveUserConfigUpdate } from "./configuration-update.ts"

export const configurationSettingsSectionMetadata = {
  id: "settings-configuration",
  get description() {
    return t`Edit user-scoped defaults for the Goddard runtime.`
  },
  get title() {
    return t`Configuration`
  },
}

type ConfigurationDocuments = {
  canonical: UserConfigDocument;
  draft: UserConfigDocument;
}

function ConfigurationSettingsForm() @{
  const config = useQuery(goddardSdk.config.get, { params: [] })
  const [documents, setDocuments] = useState<ConfigurationDocuments>({
    canonical: config.document,
    draft: config.document,
  })
  const [errors, setErrors] = useState<ErrorSchema<UserConfigDocument>>()
  const [formError, setFormError] = useState<string | null>(null)
  const [pending, setPending] = useState(false)
  const [restartRequired, setRestartRequired] = useState(false)

  useEffect(() => {
    setDocuments({
      canonical: config.document,
      draft: config.document,
    })
    setErrors(undefined)
    setFormError(null)
  }, [config.document])

  const persistDocument = async (nextDocument: UserConfigDocument) => {
    const update = deriveUserConfigUpdate(documents.canonical, nextDocument)
    setDocuments((current) => ({ ...current, draft: nextDocument }))
    setErrors(undefined)
    setFormError(null)

    if (!update) {
      if (JSON.stringify(documents.canonical) !== JSON.stringify(nextDocument)) {
        setFormError(t`Resolve the invalid setting before editing another section.`)
      }
      return
    }

    setPending(true)
    try {
      const response = await goddardSdk.config.update(update)
      setDocuments({
        canonical: response.document,
        draft: response.document,
      })
      setRestartRequired((required) => required || response.restartRequired)
    } catch (error) {
      const configError = formatConfigurationError(error)
      setFormError(configError.form)
      setErrors(
        configError.paths.length > 0
          ? createConfigErrorSchema(configError.paths, t`This value is not valid.`)
          : undefined,
      )
    } finally {
      setPending(false)
    }
  }

  <div class={styles.formSurface} aria-busy={pending ? "true" : undefined}>
    <ConfigurationForm
      disabled={pending}
      document={documents.draft}
      errors={errors}
      schema={config.schema as RJSFSchema}
      onDocumentChange={(document) => {
        void persistDocument(document)
      }}
    />

    @if (formError) {
      <p class={styles.errorText} role="alert">{formError}</p>
    }
    @if (restartRequired) {
      <p class={styles.restartNotice} role="status">
        {t`Restart the daemon to apply the saved daemon settings.`}
      </p>
    }
  </div>
}

export function ConfigurationSettingsSection() @{
  <SettingsSection
    id={configurationSettingsSectionMetadata.id}
    aria-labelledby="settings-configuration-heading"
  >
    <SettingsSectionHeader>
      <SettingsSectionTitle
        id="settings-configuration-heading"
      >{configurationSettingsSectionMetadata.title}</SettingsSectionTitle>
      <SettingsSectionDescription>{configurationSettingsSectionMetadata.description}</SettingsSectionDescription>
    </SettingsSectionHeader>
    <SettingsFieldset>
      <SettingsLegend>
        {t`User configuration`}
      </SettingsLegend>
      <SettingsHelperText>
        {t`Valid changes are saved immediately and apply to future work.`}
      </SettingsHelperText>

      @try {
        <ConfigurationSettingsForm />
      } @pending {
        <p class={styles.stateText}>
          {t`Loading configuration.`}
        </p>
      } @catch (_error, retry) {
        useErrorBoundaryReset(retry)

        <div class={styles.loadError} role="alert">
          <p class={styles.stateText}>
            {t`Configuration could not be loaded.`}
          </p>
          <button class={styles.retryButton} type="button" onClick={retry}>
            <RefreshCw aria-hidden={true} size={14} strokeWidth={2.2} />
            {t`Retry`}
          </button>
        </div>
      }
    </SettingsFieldset>
  </SettingsSection>
}

function formatConfigurationError(error: unknown) {
  if (!isIpcClientErrorForRegistry(error, UserConfigIpcErrors)) {
    return { form: t`Configuration could not be saved.`, paths: [] }
  }

  return formatKnownConfigurationError(error)
}

function formatKnownConfigurationError(error: IpcClientErrorForRegistry<typeof UserConfigIpcErrors>) {
  switch (error.code) {
    case UserConfigErrorCodes.InvalidDocument:
      return {
        form: error.details.paths.includes("") ? t`Configuration is not valid.` : null,
        paths: error.details.paths.filter(Boolean),
      }
    case UserConfigErrorCodes.InvalidPatch:
      return { form: t`This setting could not be updated.`, paths: [error.details.path] }
    case UserConfigErrorCodes.Unavailable:
      return { form: t`Configuration could not be saved.`, paths: [] }
  }
}

function createConfigErrorSchema(paths: string[], message: string) {
  const errorSchema: Record<string, unknown> = {}

  for (const pointer of paths) {
    let current = errorSchema
    for (const segment of decodeJsonPointer(pointer)) {
      const child = current[segment]
      if (typeof child !== "object" || child === null || Array.isArray(child)) {
        current[segment] = {}
      }
      current = current[segment] as Record<string, unknown>
    }
    current.__errors = [message]
  }

  return errorSchema as ErrorSchema<UserConfigDocument>
}

function decodeJsonPointer(pointer: string) {
  return pointer.slice(1).split("/").map(
    (segment) => segment.replaceAll("~1", "/").replaceAll("~0", "~"),
  )
}
