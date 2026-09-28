import { useQueryClient } from '@tanstack/react-query'
import type {
  DaemonSettings,
  InferenceProvider,
  Project,
  ProviderKind,
} from '@waku/client'
import { useEffect, useState, type ReactNode } from 'react'
import { toast } from 'sonner'
import { ControlMenu } from '@/components/control-menu'
import { SkillsSettings } from '@/components/skills-settings'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { UsageSettings } from '@/components/usage-settings'
import { ProviderIcon, PROVIDERS, WakuIcon, type WakuIconName } from '@/components/waku-icon'
import {
  useDaemonSettings,
  useProviderProbes,
} from '@/hooks/use-daemon-data'
import { useCopyFeedback } from '@/hooks/use-copy-feedback'
import {
  daemonKeys,
  updateDaemonSettings,
} from '@/lib/daemon-api'
import { useDaemon } from '@/lib/daemon-context'
import {
  applyThemeChoice,
  readThemeChoice,
  type ThemeChoice,
} from '@/lib/appearance'
import {
  APP_LANGUAGES,
  languageLabel,
  useI18n,
} from '@/lib/i18n'
import { cn } from '@/lib/utils'

export type SettingsPageId =
  | 'general'
  | 'appearance'
  | 'providers'
  | 'skills'
  | 'usage'
  | 'daemon'

export const SETTINGS_PAGES: Array<{
  id: SettingsPageId
  label: string
  labelKey: string
  icon: WakuIconName
  keywords: string
  keywordsKey: string
}> = [
  { id: 'general', label: 'General', labelKey: 'settings.general', icon: 'settings', keywords: 'general local projects conversations privacy analytics telemetry anonymous sharing', keywordsKey: 'settings.general_keywords' },
  { id: 'appearance', label: 'Appearance', labelKey: 'settings.appearance', icon: 'appearance', keywords: 'appearance theme system light dark language', keywordsKey: 'settings.appearance_keywords' },
  { id: 'providers', label: 'Providers', labelKey: 'settings.providers', icon: 'bot', keywords: 'providers agents models cli version install detect claude codex cursor opencode opencode2 amp grok pi omp oh my pi kimi fx deepseek devin droid', keywordsKey: 'settings.providers_keywords' },
  { id: 'skills', label: 'Skills', labelKey: 'settings.skills', icon: 'package', keywords: 'skills library agent disable enable delete shared', keywordsKey: 'settings.skills_keywords' },
  { id: 'usage', label: 'Usage', labelKey: 'settings.usage', icon: 'chartColumn', keywords: 'usage tokens cost spend cache daily monthly project model history', keywordsKey: 'settings.usage_keywords' },
  { id: 'daemon', label: 'Daemon', labelKey: 'settings.daemon', icon: 'server', keywords: 'daemon server remote web network connection url token websocket', keywordsKey: 'settings.daemon_keywords' },
]

export function isSettingsPageId(value: string): value is SettingsPageId {
  return SETTINGS_PAGES.some((page) => page.id === value)
}

export function SettingsView({
  page,
  projects,
  onBack,
  onPageChange,
}: {
  page: SettingsPageId
  projects: Project[]
  onBack: () => void
  onPageChange: (page: SettingsPageId) => void
}) {
  const { t } = useI18n()
  const [query, setQuery] = useState('')
  const localizedPages = SETTINGS_PAGES.map((candidate) => ({
    ...candidate,
    localizedLabel: t(candidate.labelKey),
    localizedKeywords: `${candidate.keywords} ${t(candidate.keywordsKey)}`.toLowerCase(),
  }))
  const pages = localizedPages.filter((candidate) =>
    !query.trim() || candidate.localizedKeywords.includes(query.trim().toLowerCase()),
  )
  const activePage = localizedPages.find((candidate) => candidate.id === page)

  useEffect(() => {
    const escape = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && query) setQuery('')
    }
    window.addEventListener('keydown', escape)
    return () => window.removeEventListener('keydown', escape)
  }, [query])

  return (
    <div className="flex h-dvh min-w-0 flex-1 bg-background">
      <aside className="flex h-full w-[252px] shrink-0 flex-col bg-sidebar pt-3">
        <div className="px-3">
          <button
            className="flex h-[34px] w-full items-center gap-[9px] rounded-lg px-[9px] text-[13px] text-[var(--text-secondary)] outline-none hover:bg-sidebar-accent active:bg-accent focus-visible:ring-1 focus-visible:ring-ring"
            type="button"
            onClick={onBack}
          >
            <WakuIcon className="size-[15px] text-[var(--text-tertiary)]" name="arrowLeft" />
            {t('settings.back')}
          </button>
        </div>
        <div className="px-3 pt-2">
          <label className="flex h-8 items-center gap-2 rounded-lg border bg-[var(--inset)] px-2.5 focus-within:border-ring">
            <WakuIcon className="size-[13px] text-[var(--text-tertiary)]" name="search" />
            <input
              aria-label={t('settings.search')}
              className="min-w-0 flex-1 bg-transparent text-[12px] outline-none placeholder:text-[var(--text-ghost)]"
              placeholder={t('settings.search')}
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              onKeyDown={(event) => {
                if (!pages.length || !['ArrowDown', 'ArrowUp'].includes(event.key)) return
                event.preventDefault()
                const current = pages.findIndex((candidate) => candidate.id === page)
                const delta = event.key === 'ArrowDown' ? 1 : -1
                onPageChange(pages[(current + delta + pages.length) % pages.length]!.id)
              }}
            />
          </label>
        </div>
        <nav aria-label={t('common.settings')} className="mt-[18px] flex flex-col gap-[3px] px-3">
          {pages.map((candidate) => (
            <button
              aria-current={page === candidate.id ? 'page' : undefined}
              className={cn(
                'flex h-9 items-center gap-2.5 rounded-lg px-[11px] text-[13px] text-[var(--text-secondary)] outline-none hover:bg-sidebar-accent focus-visible:ring-1 focus-visible:ring-ring',
                page === candidate.id && 'bg-sidebar-accent text-foreground',
              )}
              key={candidate.id}
              type="button"
              onClick={() => onPageChange(candidate.id)}
            >
              <WakuIcon className="size-[15px] text-[var(--text-tertiary)]" name={candidate.icon} />
              {candidate.localizedLabel}
            </button>
          ))}
        </nav>
      </aside>
      <main className={cn(
        'min-w-0 flex-1 border-l bg-background',
        page === 'skills' ? 'overflow-hidden' : 'overflow-y-auto px-8 pb-12 pt-5',
      )}>
        {page === 'skills' ? (
          <SkillsSettings projects={projects} />
        ) : (
          <div className={cn('mx-auto w-full', page === 'usage' ? 'max-w-[1024px]' : 'max-w-[760px]')}>
            <h1 className="text-[18px] font-medium">{activePage?.localizedLabel}</h1>
            {page === 'general' && <GeneralSettings />}
            {page === 'appearance' && <AppearanceSettings />}
            {page === 'providers' && <ProvidersSettings />}
            {page === 'providers' && <InferenceProvidersSettings />}
            {page === 'usage' && <UsageSettings projects={projects} />}
            {page === 'daemon' && <DaemonSettings />}
          </div>
        )}
      </main>
    </div>
  )
}

function GeneralSettings() {
  const { t } = useI18n()
  const [analytics, setAnalytics] = useStoredBoolean('waku.analytics-enabled', true)
  return (
    <div>
      <SettingsCard>
        <SettingText
          icon="local"
          title={t('settings.local_by_default')}
          description={t('settings.local_by_default_web_description')}
        />
      </SettingsCard>
      <SettingsCard row>
        <SettingText
          icon="chartColumn"
          title={t('settings.share_anonymous_usage_data')}
          description={t('settings.share_anonymous_usage_data_description')}
        />
        <Toggle checked={analytics} label={t('settings.share_anonymous_usage_data')} onChange={setAnalytics} />
      </SettingsCard>
    </div>
  )
}

function AppearanceSettings() {
  const { language, locale, setLanguage, t } = useI18n()
  const [theme, setTheme] = useState<ThemeChoice>(() => typeof window === 'undefined'
    ? 'system'
    : readThemeChoice(window.localStorage))
  useEffect(() => {
    const systemAppearance = window.matchMedia('(prefers-color-scheme: dark)')
    const apply = () => applyThemeChoice(document.documentElement, theme, systemAppearance.matches)
    apply()
    window.localStorage.setItem('waku.theme', theme)
    systemAppearance.addEventListener('change', apply)
    return () => systemAppearance.removeEventListener('change', apply)
  }, [theme])
  const themeLabel = t(`settings.theme_${theme}`)
  return (
    <div className="mt-[15px] w-full overflow-hidden rounded-[13px] bg-[var(--raised)]">
      <div className="flex min-h-[60px] items-center gap-6 px-5 py-3">
        <SettingText icon="appearance" title={t('settings.theme')} description={t('settings.theme_description')} />
        <ControlMenu
          align="right"
          items={(['system', 'light', 'dark'] as ThemeChoice[]).map((choice) => ({
            id: choice,
            label: t(`settings.theme_${choice}`),
            selected: choice === theme,
            onSelect: () => setTheme(choice),
          }))}
          label={themeLabel}
          menuClassName="w-[140px]"
          placement="below"
          triggerClassName="h-8 w-[116px] max-w-none justify-between border bg-background px-3 text-[12px]"
        />
      </div>
      <div className="mx-5 border-t" />
      <div className="flex min-h-[60px] items-center gap-6 px-5 py-3">
        <SettingText icon="languages" title={t('language.title')} description={t('language.description')} />
        <ControlMenu
          align="right"
          items={APP_LANGUAGES.map((choice) => ({
            id: choice,
            label: languageLabel(choice, locale),
            selected: choice === language,
            onSelect: () => setLanguage(choice),
          }))}
          label={languageLabel(language, locale)}
          menuClassName="w-[160px]"
          placement="below"
          triggerClassName="h-8 w-[116px] max-w-none justify-between border bg-background px-3 text-[12px]"
        />
      </div>
    </div>
  )
}

function ProvidersSettings() {
  const { t } = useI18n()
  const { client, config } = useDaemon()
  const queryClient = useQueryClient()
  const settings = useDaemonSettings()
  const probes = useProviderProbes()
  const [expanded, setExpanded] = useState<ProviderKind | null>(null)
  const [paths, setPaths] = useState<Partial<Record<ProviderKind, string>>>({})
  const checkedAt = Math.max(
    0,
    ...Object.values(probes.states).map((state) => state.dataUpdatedAt),
  )

  async function apply(next: DaemonSettings) {
    if (!client || !config) return
    try {
      await updateDaemonSettings(client, next)
      queryClient.setQueryData(daemonKeys.settings(config.address), next)
      await queryClient.invalidateQueries({ queryKey: daemonKeys.providers(config.address) })
    } catch (error) {
      toast.error(errorMessage(error))
    }
  }

  function applyProviderPath(provider: ProviderKind, value: string) {
    if (!settings.data) return
    const overrides = { ...(settings.data.provider_binary_overrides ?? {}) }
    const trimmed = value.trim()
    if (trimmed) overrides[provider] = trimmed
    else delete overrides[provider]
    setPaths((current) => ({ ...current, [provider]: trimmed }))
    void apply({ ...settings.data, provider_binary_overrides: overrides })
  }

  function toggleExpandedProvider(provider: ProviderKind) {
    if (expanded) {
      const pending = paths[expanded]
      const applied = settings.data?.provider_binary_overrides?.[expanded] ?? ''
      if (pending !== undefined && pending.trim() !== applied) {
        applyProviderPath(expanded, pending)
      }
    }
    if (expanded !== provider) {
      setPaths((current) => ({
        ...current,
        [provider]: settings.data?.provider_binary_overrides?.[provider] ?? '',
      }))
    }
    setExpanded(expanded === provider ? null : provider)
  }

  return (
    <div className="mt-[15px] overflow-hidden rounded-[13px] bg-[var(--raised)] px-5 py-[14px]">
      <div className="flex items-center gap-3">
        <span className="grid w-5 shrink-0 place-items-center">
          <WakuIcon className="size-4 text-[var(--text-tertiary)]" name="bot" />
        </span>
        <div className="min-w-0 flex-1">
          <div className="text-[13.5px] font-medium">{t('providers.coding_agents')}</div>
          <p className="mt-[5px] text-[12px] leading-[18px] text-[var(--text-secondary)]">
            {t('providers.web_description')}
          </p>
        </div>
        <div className="flex shrink-0 flex-col items-end gap-1.5">
          <button
            className="flex h-7 items-center gap-1.5 rounded-[7px] border border-input px-[11px] text-[10.5px] text-[var(--text-secondary)] outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-60"
            disabled={probes.isFetching}
            type="button"
            onClick={() => {
              if (!config) return
              void queryClient.invalidateQueries({ queryKey: daemonKeys.providers(config.address) })
            }}
          >
            <WakuIcon className={cn('size-[11px] text-[var(--text-tertiary)]', probes.isFetching && 'motion-safe:animate-spin')} name="rotateCw" />
            {probes.isFetching ? t('common.checking') : t('common.refresh')}
          </button>
          {!probes.isFetching && checkedAt > 0 && (
            <span className="text-[9.5px] text-[var(--text-ghost)]">{providerCheckedLabel(checkedAt, t)}</span>
          )}
        </div>
      </div>
      <div className="mt-1 flex flex-col">
        {PROVIDERS.map((provider) => {
          const probe = probes.data[provider.id]
          const probeState = probes.states[provider.id]
          const installed = probe?.installed ?? false
          const disabled = settings.data?.disabled_providers.includes(provider.id) ?? false
          const open = expanded === provider.id
          const detail = providerProbeDetail(provider.command, probe, probeState, disabled, t)
          const dotColor = probeState.error
            ? 'bg-[var(--warning)]'
            : !installed
              ? 'bg-[var(--text-ghost)]'
              : disabled
                ? 'bg-[var(--warning)]'
                : 'bg-[var(--success)]'
          return (
            <div className="border-b last:border-0" key={provider.id}>
              <div className="flex items-center gap-3 py-[11px]">
                <span className="relative grid size-[30px] shrink-0 place-items-center rounded-[7px] bg-accent">
                  <ProviderIcon className={cn('size-4', !installed && 'opacity-50')} provider={provider.id} />
                  <span className={cn('absolute -bottom-0.5 -right-0.5 size-2.5 rounded-full border-2 border-[var(--raised)]', dotColor)} />
                </span>
                <span className="min-w-0 flex-1">
                  <span className="flex items-baseline gap-[7px]">
                    <span className={cn('truncate text-[12.5px] font-medium', !installed && 'text-[var(--text-secondary)]')}>{provider.name}</span>
                    {probe?.version && (
                      <span className="shrink-0 font-mono text-[10px] text-[var(--text-tertiary)]">v{probe.version}</span>
                    )}
                  </span>
                  <span className="mt-[3px] block truncate text-[10.5px] text-[var(--text-tertiary)]" title={detail}>{detail}</span>
                </span>
                <button
                  aria-expanded={open}
                  aria-label={t(open ? 'providers.hide_settings' : 'providers.show_settings', { provider: provider.name })}
                  className="grid size-7 shrink-0 place-items-center rounded-[7px] text-[var(--text-tertiary)] outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring"
                  type="button"
                  onClick={() => toggleExpandedProvider(provider.id)}
                >
                  <WakuIcon className="size-2.5" name={open ? 'chevronDown' : 'chevronRight'} />
                </button>
                {installed && (
                  <Toggle
                    checked={!disabled}
                    label={t(disabled ? 'providers.enable' : 'providers.disable', { provider: provider.name })}
                    onChange={(enabled) => {
                      if (!settings.data) return
                      const disabledProviders = enabled
                        ? settings.data.disabled_providers.filter((kind) => kind !== provider.id)
                        : [...new Set([...settings.data.disabled_providers, provider.id])]
                      void apply({ ...settings.data, disabled_providers: disabledProviders })
                    }}
                  />
                )}
              </div>
              {open && settings.data && (
                <div className="mb-[11px] ml-[42px] flex flex-col gap-[5px]">
                  <label className="text-[11.5px] font-medium">{t('providers.binary_path')}</label>
                  <p className="text-[10.5px] leading-[15px] text-[var(--text-tertiary)]">
                    {t('providers.binary_path_description', { provider: provider.shortName })}
                  </p>
                  <div className="mt-[3px] flex items-center gap-2">
                    <Input
                      autoFocus
                      className="h-[29px] max-w-[430px] flex-1 bg-[var(--inset)] font-mono text-[11px]"
                      placeholder={t('input.detected_automatically')}
                      value={paths[provider.id] ?? settings.data.provider_binary_overrides?.[provider.id] ?? ''}
                      onChange={(event) => setPaths((current) => ({ ...current, [provider.id]: event.target.value }))}
                      onKeyDown={(event) => {
                        if (event.key !== 'Enter') return
                        applyProviderPath(provider.id, event.currentTarget.value)
                      }}
                    />
                    {settings.data.provider_binary_overrides?.[provider.id] && (
                      <button
                        className="h-[29px] shrink-0 rounded-[7px] border border-input px-2.5 text-[10.5px] text-[var(--text-secondary)] outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring"
                        type="button"
                        onClick={() => applyProviderPath(provider.id, '')}
                      >
                        {t('common.reset')}
                      </button>
                    )}
                  </div>
                  <p className="truncate text-[10px] text-[var(--text-ghost)]" title={providerProbeCaption(provider.command, probe, Boolean(settings.data.provider_binary_overrides?.[provider.id]), t)}>
                    {providerProbeCaption(provider.command, probe, Boolean(settings.data.provider_binary_overrides?.[provider.id]), t)}
                  </p>
                </div>
              )}
            </div>
          )
        })}
      </div>
    </div>
  )
}

type InferenceProviderSpec = {
  id: InferenceProvider
  name: string
  icon: WakuIconName
  contexts: Array<'eval' | 'text' | 'speech'>
  keysUrl: string
  config: Array<{ field: string; labelKey: string; descriptionKey: string; placeholderKey: string }>
}

// Mirrors `InferenceProvider`'s static spec in waku-protocol — kept in sync
// by hand the same way PROVIDERS mirrors ProviderKind.
const INFERENCE_PROVIDERS: InferenceProviderSpec[] = [
  {
    id: 'typeSafe',
    name: 'TypeSafe',
    icon: 'sparkle',
    contexts: ['eval'],
    keysUrl: 'https://typesafe.ai',
    config: [],
  },
  {
    id: 'vercelGateway',
    name: 'Vercel AI Gateway',
    icon: 'zap',
    contexts: ['eval', 'text', 'speech'],
    keysUrl: 'https://vercel.com/docs/ai-gateway',
    config: [
      {
        field: 'team_id',
        labelKey: 'routing.vercel_team',
        descriptionKey: 'routing.vercel_team_description',
        placeholderKey: 'routing.optional',
      },
    ],
  },
  {
    id: 'cloudflare',
    name: 'Cloudflare Workers AI',
    icon: 'server',
    contexts: ['eval', 'text'],
    keysUrl: 'https://developers.cloudflare.com/workers-ai/get-started/rest-api/',
    config: [
      {
        field: 'account_id',
        labelKey: 'routing.cloudflare_account',
        descriptionKey: 'routing.cloudflare_account_description',
        placeholderKey: 'routing.cloudflare_account_placeholder',
      },
    ],
  },
  {
    id: 'openRouter',
    name: 'OpenRouter',
    icon: 'globe',
    contexts: ['text', 'speech'],
    keysUrl: 'https://openrouter.ai/settings/keys',
    config: [],
  },
]

const INFERENCE_CONTEXT_LABEL_KEY = {
  eval: 'inference.context_eval',
  text: 'inference.context_text',
  speech: 'inference.context_speech',
} as const

function InferenceProvidersSettings() {
  const { t } = useI18n()
  const { client, config } = useDaemon()
  const queryClient = useQueryClient()
  const settings = useDaemonSettings()
  const [expanded, setExpanded] = useState<InferenceProvider | null>(null)
  const [drafts, setDrafts] = useState<Partial<Record<InferenceProvider, { key: string; config: Record<string, string> }>>>({})

  async function apply(next: DaemonSettings) {
    if (!client || !config) return
    try {
      await updateDaemonSettings(client, next)
      queryClient.setQueryData(daemonKeys.settings(config.address), next)
    } catch (error) {
      toast.error(errorMessage(error))
    }
  }

  function draftFor(provider: InferenceProvider) {
    const entry = settings.data?.inference?.[provider]
    return (
      drafts[provider] ?? {
        key: '',
        config: { ...(entry?.config ?? {}) },
      }
    )
  }

  function dirty(provider: InferenceProvider) {
    const draft = drafts[provider]
    if (!draft) return false
    if (draft.key.trim()) return true
    const stored = settings.data?.inference?.[provider]?.config ?? {}
    return Object.keys({ ...stored, ...draft.config }).some(
      (field) => (draft.config[field] ?? '') !== (stored[field] ?? ''),
    )
  }

  function applyProvider(provider: InferenceProvider) {
    if (!settings.data) return
    const draft = draftFor(provider)
    const key = draft.key.trim()
    const config = Object.fromEntries(
      Object.entries(draft.config)
        .map(([field, value]) => [field, value.trim()])
        .filter(([, value]) => value !== ''),
    )
    const inference = { ...(settings.data.inference ?? {}) }
    inference[provider] = {
      // The daemon absorbs a non-empty key into its secret store; an empty
      // string removes it. `null` leaves the stored credential untouched.
      apiKey: key ? key : null,
      credentialConfigured: settings.data.inference?.[provider]?.credentialConfigured ?? false,
      config,
    }
    setDrafts((current) => ({ ...current, [provider]: { key: '', config } }))
    void apply({ ...settings.data, inference })
  }

  function clearCredential(provider: InferenceProvider) {
    if (!settings.data) return
    const inference = { ...(settings.data.inference ?? {}) }
    inference[provider] = {
      ...inference[provider],
      apiKey: '',
      credentialConfigured: false,
      config: inference[provider]?.config ?? {},
    }
    void apply({ ...settings.data, inference })
  }

  return (
    <div className="mt-[15px] overflow-hidden rounded-[13px] bg-[var(--raised)] px-5 py-[14px]">
      <div className="flex items-center gap-3">
        <span className="grid w-5 shrink-0 place-items-center">
          <WakuIcon className="size-4 text-[var(--text-tertiary)]" name="zap" />
        </span>
        <div className="min-w-0 flex-1">
          <div className="text-[13.5px] font-medium">{t('inference.providers')}</div>
          <p className="mt-[5px] text-[12px] leading-[18px] text-[var(--text-secondary)]">
            {t('inference.description')}
          </p>
        </div>
      </div>
      <div className="mt-1 flex flex-col">
        {INFERENCE_PROVIDERS.map((provider) => {
          const entry = settings.data?.inference?.[provider.id]
          const configured = Boolean(entry?.credentialConfigured)
          const open = expanded === provider.id
          const contexts = provider.contexts.map((context) => t(INFERENCE_CONTEXT_LABEL_KEY[context])).join('  ·  ')
          const detail = `${contexts}  ·  ${configured ? t('inference.key_configured') : t('inference.no_key')}`
          const draft = draftFor(provider.id)
          return (
            <div className="border-b last:border-0" key={provider.id}>
              <div className="flex items-center gap-3 py-[11px]">
                <span className="grid size-[30px] shrink-0 place-items-center rounded-[7px] bg-accent">
                  <WakuIcon className="size-4 text-[var(--text-tertiary)]" name={provider.icon} />
                </span>
                <span className="min-w-0 flex-1">
                  <span className="truncate text-[12.5px] font-medium">{provider.name}</span>
                  <span className="mt-[3px] block truncate text-[10.5px] text-[var(--text-tertiary)]" title={detail}>{detail}</span>
                </span>
                <span className={cn('text-[10.5px]', configured ? 'text-[var(--success)]' : 'text-[var(--text-ghost)]')}>
                  {configured ? t('inference.configured') : t('inference.not_configured')}
                </span>
                <button
                  aria-expanded={open}
                  aria-label={t(open ? 'providers.hide_settings' : 'providers.show_settings', { provider: provider.name })}
                  className="grid size-7 shrink-0 place-items-center rounded-[7px] text-[var(--text-tertiary)] outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring"
                  type="button"
                  onClick={() => setExpanded(open ? null : provider.id)}
                >
                  <WakuIcon className="size-2.5" name={open ? 'chevronDown' : 'chevronRight'} />
                </button>
              </div>
              {open && (
                <div className="mb-[11px] ml-[42px] flex flex-col gap-[5px]">
                  <label className="text-[11.5px] font-medium">{t('inference.api_key')}</label>
                  <p className="text-[10.5px] leading-[15px] text-[var(--text-tertiary)]">
                    {configured ? t('inference.api_key_stored_description') : t('inference.api_key_description')}
                  </p>
                  <div className="mt-[3px] flex items-center gap-2">
                    <Input
                      autoComplete="off"
                      className="h-[29px] max-w-[430px] flex-1 bg-[var(--inset)] font-mono text-[11px]"
                      type="password"
                      value={draft.key}
                      onChange={(event) =>
                        setDrafts((current) => ({
                          ...current,
                          [provider.id]: { ...draftFor(provider.id), key: event.target.value },
                        }))
                      }
                      onKeyDown={(event) => {
                        if (event.key !== 'Enter') return
                        applyProvider(provider.id)
                      }}
                    />
                    <a
                      className="flex h-[29px] shrink-0 items-center gap-1.5 rounded-[7px] border border-input px-2.5 text-[10.5px] text-[var(--text-secondary)] outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring"
                      href={provider.keysUrl}
                      rel="noreferrer"
                      target="_blank"
                    >
                      <WakuIcon className="size-[11px] text-[var(--text-tertiary)]" name="arrowUpRight" />
                      {t('inference.get_key')}
                    </a>
                    {configured && (
                      <button
                        className="h-[29px] shrink-0 rounded-[7px] border border-input px-2.5 text-[10.5px] text-[var(--text-secondary)] outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring"
                        type="button"
                        onClick={() => clearCredential(provider.id)}
                      >
                        {t('inference.remove_key')}
                      </button>
                    )}
                  </div>
                  {provider.config.map((field) => (
                    <div className="flex flex-col gap-[5px]" key={field.field}>
                      <label className="mt-[6px] text-[11.5px] font-medium">{t(field.labelKey)}</label>
                      <p className="text-[10.5px] leading-[15px] text-[var(--text-tertiary)]">{t(field.descriptionKey)}</p>
                      <Input
                        className="mt-[3px] h-[29px] max-w-[430px] flex-1 bg-[var(--inset)] font-mono text-[11px]"
                        placeholder={t(field.placeholderKey)}
                        value={draft.config[field.field] ?? ''}
                        onChange={(event) =>
                          setDrafts((current) => ({
                            ...current,
                            [provider.id]: {
                              ...draftFor(provider.id),
                              config: { ...draftFor(provider.id).config, [field.field]: event.target.value },
                            },
                          }))
                        }
                        onKeyDown={(event) => {
                          if (event.key !== 'Enter') return
                          applyProvider(provider.id)
                        }}
                      />
                    </div>
                  ))}
                  <div className="mt-[6px] flex items-center gap-2">
                    <button
                      className="h-[29px] shrink-0 rounded-[7px] border border-input px-2.5 text-[10.5px] text-[var(--text-secondary)] outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50"
                      disabled={!dirty(provider.id)}
                      type="button"
                      onClick={() => applyProvider(provider.id)}
                    >
                      {t('daemon.apply')}
                    </button>
                    <span className="text-[10px] text-[var(--text-ghost)]">{t('inference.apply_hint')}</span>
                  </div>
                </div>
              )}
            </div>
          )
        })}
      </div>
    </div>
  )
}

function providerProbeDetail(
  command: string,
  probe: ReturnType<typeof useProviderProbes>['data'][ProviderKind],
  state: ReturnType<typeof useProviderProbes>['states'][ProviderKind],
  disabled: boolean,
  t: Translator,
) {
  if (state.isPending) return t('common.checking')
  if (state.error) return t('providers.check_failed', { command, error: errorMessage(state.error) })
  if (!probe?.installed) return t('providers.not_detected_as', { command })
  const parts = []
  if (probe.path) parts.push(abbreviateHomePath(probe.path))
  if (disabled) parts.push(t('providers.disabled_for_new_tasks'))
  else if (probe.models.length) parts.push(t(
    probe.models.length === 1 ? 'providers.model_count_one' : 'providers.model_count_many',
    { count: probe.models.length },
  ))
  return parts.join('  ·  ') || t('providers.detected_as', { command })
}

function providerProbeCaption(
  command: string,
  probe: ReturnType<typeof useProviderProbes>['data'][ProviderKind],
  hasOverride: boolean,
  t: Translator,
) {
  if (hasOverride && probe?.installed && probe.path) return t('providers.using_override', { path: probe.path })
  if (hasOverride) return t('providers.invalid_override')
  if (probe?.installed && probe.path) return t('providers.detected_at', { path: probe.path })
  return t('providers.searches_path', { command })
}

function providerCheckedLabel(updatedAt: number, t: Translator) {
  const seconds = Math.max(0, Math.floor((Date.now() - updatedAt) / 1_000))
  if (seconds < 60) return t('providers.checked_just_now')
  const minutes = Math.floor(seconds / 60)
  if (minutes < 60) return t('providers.checked_minutes_ago', { count: minutes })
  const hours = Math.floor(minutes / 60)
  return t('providers.checked_hours_ago', { count: hours })
}

type Translator = (key: string, params?: Record<string, string | number>) => string

function abbreviateHomePath(path: string) {
  return path
    .replace(/^\/Users\/[^/]+(?=\/|$)/, '~')
    .replace(/^\/home\/[^/]+(?=\/|$)/, '~')
    .replace(/^\/root(?=\/|$)/, '~')
}

function DaemonSettings() {
  const { t } = useI18n()
  const { client, config, phase, reconnect, disconnect, forget } = useDaemon()
  const settings = useDaemonSettings()
  const queryClient = useQueryClient()
  const [error, setError] = useState<string | null>(null)

  async function setAgentTools(enabled: boolean) {
    if (!client || !config || !settings.data) return
    const next = { ...settings.data, agent_tools_enabled: enabled }
    try {
      await updateDaemonSettings(client, next)
      queryClient.setQueryData(daemonKeys.settings(config.address), next)
    } catch (cause) {
      setError(errorMessage(cause))
    }
  }

  return (
    <div>
      <SettingsCard>
        <SettingText
          icon="unplug"
          title={t('daemon.external_title')}
          description={t('daemon.web_external_description')}
        />
      </SettingsCard>
      <SettingsCard row>
        <SettingText
          icon="bot"
          title={t('daemon.agent_tools_title')}
          description={t('daemon.agent_tools_description')}
        />
        <Toggle
          checked={settings.data?.agent_tools_enabled ?? false}
          label={t('daemon.agent_tools_title')}
          onChange={(enabled) => void setAgentTools(enabled)}
        />
      </SettingsCard>
      <SettingsCard>
        <SettingText icon="keyRound" title={t('daemon.credentials_title')} description={t('daemon.web_connection_description')} />
        <div className="mt-4 divide-y rounded-xl border bg-background px-3">
          <DetailRow icon="link" label={t('daemon.websocket_url')} value={config?.address ?? t('daemon.not_configured')} copy />
          <DetailRow
            copy={Boolean(config?.token)}
            icon="keyRound"
            label={t('daemon.token')}
            secret={Boolean(config?.token)}
            value={config?.token ?? t('daemon.not_configured')}
          />
          <DetailRow icon="wifi" label={t('daemon.status')} value={t(`daemon.phase_${phase}`)} />
        </div>
        {error && <p className="mt-3 text-[11.5px] text-destructive">{error}</p>}
        <div className="mt-4 flex flex-wrap justify-end gap-2">
          <Button variant="ghost" onClick={disconnect}>{t('daemon.disconnect')}</Button>
          <Button variant="destructive" onClick={forget}>{t('daemon.forget')}</Button>
          <Button
            disabled={phase === 'connecting'}
            onClick={() => {
              setError(null)
              void reconnect().catch((cause) => setError(errorMessage(cause)))
            }}
          >
            {t('daemon.reconnect')}
          </Button>
        </div>
      </SettingsCard>
    </div>
  )
}

function SettingsCard({ children, row = false }: { children: ReactNode; row?: boolean }) {
  return (
    <section className={cn('mt-[15px] w-full rounded-[13px] bg-[var(--raised)] px-5 py-[14px]', row && 'flex items-center gap-6')}>
      {children}
    </section>
  )
}

function SettingText({
  icon,
  title,
  description,
}: {
  icon: WakuIconName
  title: string
  description: string
}) {
  return (
    <div className="flex min-w-0 flex-1 items-center gap-3">
      <span className="grid w-5 shrink-0 place-items-center">
        <WakuIcon className="size-4 text-[var(--text-tertiary)]" name={icon} />
      </span>
      <div className="min-w-0 flex-1">
        <div className="text-[13.5px] font-medium">{title}</div>
        <p className="mt-[5px] text-[12.5px] leading-[18px] text-[var(--text-secondary)]">{description}</p>
      </div>
    </div>
  )
}

function Toggle({ checked, label, onChange }: { checked: boolean; label: string; onChange: (checked: boolean) => void }) {
  return (
    <button
      aria-checked={checked}
      aria-label={label}
      className={cn(
        'flex h-5 w-9 shrink-0 items-center rounded-full border p-0.5 outline-none transition-colors focus-visible:ring-1 focus-visible:ring-ring',
        checked ? 'justify-end border-foreground bg-foreground' : 'justify-start border-input bg-[var(--inset)]',
      )}
      role="switch"
      type="button"
      onClick={() => onChange(!checked)}
    >
      <span className={cn('size-3.5 rounded-full', checked ? 'bg-background' : 'bg-[var(--text-tertiary)]')} />
    </button>
  )
}

function DetailRow({
  icon,
  label,
  value,
  copy = false,
  secret = false,
}: {
  icon: WakuIconName
  label: string
  value: string
  copy?: boolean
  secret?: boolean
}) {
  const { t } = useI18n()
  const copyFeedback = useCopyFeedback()
  const [revealed, setRevealed] = useState(false)
  return (
    <div className="flex min-h-12 items-center gap-4 text-[11.5px]">
      <span className="grid w-5 shrink-0 place-items-center">
        <WakuIcon className="size-4 text-[var(--text-tertiary)]" name={icon} />
      </span>
      <span className="w-28 shrink-0 text-[var(--text-tertiary)]">{label}</span>
      <span className="min-w-0 flex-1 truncate font-mono">
        {secret && !revealed ? '••••••••••••••••••••••••' : value}
      </span>
      {secret && (
        <Button
          aria-label={t(revealed ? 'daemon.hide_token' : 'daemon.reveal_token')}
          aria-pressed={revealed}
          size="icon-sm"
          title={t(revealed ? 'daemon.hide_token' : 'daemon.reveal_token')}
          type="button"
          variant="outline"
          onClick={() => setRevealed((current) => !current)}
        >
          <WakuIcon name={revealed ? 'eyeOff' : 'eye'} />
        </Button>
      )}
      {copy && (
        <Button size="sm" variant="outline" onClick={() => void copyFeedback.copyText(value)}>
          <WakuIcon name={copyFeedback.copied ? 'check' : 'copy'} />
          {t(copyFeedback.copied ? 'common.copied' : 'common.copy')}
        </Button>
      )}
    </div>
  )
}

function useStoredBoolean(key: string, fallback: boolean) {
  const [value, setValue] = useState(() => typeof window === 'undefined' ? fallback : window.localStorage.getItem(key) !== 'false')
  const update = (next: boolean) => {
    setValue(next)
    window.localStorage.setItem(key, String(next))
  }
  return [value, update] as const
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}
