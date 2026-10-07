import type { BossState, ProviderKind } from '@waku/client';
import { loadBossState } from '@waku/client/boss';
import {
  PROVIDER_PROBE_CACHE_STALE_TIME,
  readProviderProbeCache,
  writeProviderProbeCache,
} from '@waku/client/provider-probe-cache';

import {
  daemonKeys,
  hydrateSession,
  loadDaemonSettings,
  loadTaskState,
  probeProvider,
  type TaskState,
} from './daemon-api';

import { persistentStorageSync } from './composer-preferences-store';
import { useDaemon, useDaemonQuery, getDaemonQueryData, type DaemonQueryResult } from './daemon-hooks';
import { providerLabel } from './session-presentation';

const PROVIDERS: ProviderKind[] = [
  'codex',
  'claude',
  'cursor',
  'amp',
  'copilot',
  'openCode',
  'grok',
  'kimi',
  'deepSeek',
  'devin',
  'droid',
  'fx',
  'ohMyPi',
  'pi',
];

export function useTaskState(): DaemonQueryResult<TaskState> {
  const { activeProfile, client, phase } = useDaemon();
  return useDaemonQuery(
    daemonKeys.taskState(activeProfile?.id ?? 'disconnected'),
    () => loadTaskState(requireClient(client)),
    {
      enabled: phase === 'connected' && Boolean(activeProfile && client),
      staleTime: 1_000,
    },
  );
}

export function useSession(sessionId: string | undefined): DaemonQueryResult<AgentSessionLike> {
  const { activeProfile, client, phase } = useDaemon();
  const profileId = activeProfile?.id ?? 'disconnected';
  const query = useDaemonQuery(
    daemonKeys.session(profileId, sessionId ?? 'missing'),
    () => hydrateSession(requireClient(client), sessionId!),
    {
      enabled: phase === 'connected' && Boolean(activeProfile && client && sessionId),
      staleTime: 1_000,
    },
  );

  // react-query placeholderData: seed from the task-state session summary.
  if (query.data === undefined && sessionId) {
    const taskState = getDaemonQueryData<TaskState>(daemonKeys.taskState(profileId));
    const placeholder = taskState?.sessions.find((session) => session.id === sessionId);
    if (placeholder) {
      return { ...query, data: placeholder as AgentSessionLike };
    }
  }

  return query;
}

type AgentSessionLike = Awaited<ReturnType<typeof hydrateSession>>;

/** The daemon's Boss document — employees, plans, deliverables — or `null`
 * when the host's boss experiment is off. Re-fetches on every
 * `taskStateChanged` broadcast, the same notifier boss mutations use. */
export function useBossState(): DaemonQueryResult<BossState | null> {
  const { activeProfile, client, phase } = useDaemon();
  return useDaemonQuery(
    daemonKeys.bossState(activeProfile?.id ?? 'disconnected'),
    () => loadBossState(requireClient(client)),
    {
      enabled: phase === 'connected' && Boolean(activeProfile && client),
      staleTime: 1_000,
    },
  );
}

export function useDaemonSettings(): DaemonQueryResult<import('@waku/client').DaemonSettings> {
  const { activeProfile, client, phase } = useDaemon();
  return useDaemonQuery(
    daemonKeys.settings(activeProfile?.id ?? 'disconnected'),
    () => loadDaemonSettings(requireClient(client)),
    {
      enabled: phase === 'connected' && Boolean(activeProfile && client),
      staleTime: 60_000,
    },
  );
}

type Daemon = ReturnType<typeof useDaemon>;
type Settings = DaemonQueryResult<import('@waku/client').DaemonSettings>;

/** Web's probe-query recipe: seed from the persistent probe cache so model
 * lists render instantly across app restarts, refresh after the shared 24h
 * staleness, and write fresh probes back through the same cache. */
function useProviderModelsQuery(
  daemon: Daemon,
  settings: Settings,
  provider: ProviderKind,
): DaemonQueryResult<import('@waku/client').ProviderProbe> {
  const address = daemon.activeProfile?.address ?? 'disconnected';
  const cached = daemon.activeProfile
    ? readProviderProbeCache(persistentStorageSync(), address, provider)
    : undefined;

  const binaryOverride = settings.data
    ? settings.data.provider_binary_overrides?.[provider] ?? null
    : cached?.binaryOverride ?? null;

  const initial = cached && cached.binaryOverride === binaryOverride ? cached : undefined;
  const query = useDaemonQuery(
    [...daemonKeys.provider(daemon.activeProfile?.id ?? 'disconnected', provider), 'models'],
    async () => {
      const data = await probeProvider(requireClient(daemon.client), provider, settings.data!, {
        discoverModels: true,
        probeVersion: false,
      });

      writeProviderProbeCache(persistentStorageSync(), address, provider, binaryOverride, data);
      return data;
    },
    {
      enabled: daemon.phase === 'connected' &&
        Boolean(daemon.activeProfile && daemon.client && settings.data),
      staleTime: PROVIDER_PROBE_CACHE_STALE_TIME,
    },
  );

  if (query.data === undefined && initial) {
    return { ...query, data: initial.data, dataUpdatedAt: initial.updatedAt };
  }

  return query;
}

/** Full probe with model discovery, for the model picker. Screens mount this
 * ahead of opening the sheet so the list is warm by the time it appears. */
export function useProviderModels(provider: ProviderKind | null) {
  const daemon = useDaemon();
  const settings = useDaemonSettings();
  return useProviderModelsQuery(daemon, settings, provider ?? 'codex');
}

/** Model discovery across every given provider, cache-shared with
 * useProviderModels. Backs the cross-provider model picker. */
export function useAllProviderModels(providers: ProviderKind[]) {
  const daemon = useDaemon();
  const settings = useDaemonSettings();
  return providers.map((id) => ({
    id,
    label: providerLabel(id),
    models: useProviderModelsQuery(daemon, settings, id).data?.models ?? [],
    isPending: settings.isPending,
  }));
}

export function useProviderCatalog() {
  const { activeProfile, client, phase } = useDaemon();
  const settings = useDaemonSettings();
  const enabledProviders = PROVIDERS.filter((provider) => (
    !settings.data?.disabled_providers.includes(provider)
  ));

  const results = PROVIDERS.map((provider) => {
    const cached = activeProfile
      ? readProviderProbeCache(persistentStorageSync(), activeProfile.address, provider)
      : undefined;

    const enabled = enabledProviders.includes(provider);
    const query = useDaemonQuery(
      daemonKeys.provider(activeProfile?.id ?? 'disconnected', provider),
      () => probeProvider(requireClient(client), provider, settings.data!, {
        discoverModels: false,
        probeVersion: false,
      }),
      {
        enabled:
          enabled &&
          phase === 'connected' &&
          Boolean(activeProfile && client && settings.data),
        staleTime: 60_000,
      },
    );

    if (query.data === undefined && cached) {
      return { query: { ...query, data: cached.data, dataUpdatedAt: cached.updatedAt }, provider };
    }

    return { query, provider };
  });

  return {
    providers: enabledProviders.map((id) => {
      const { query } = results[PROVIDERS.indexOf(id)]!;
      return {
        id,
        label: providerLabel(id),
        installed: query.data?.installed === true,
        path: query.data?.path ?? null,
        isPending: query.isPending,
        error: query.error,
      };
    }),
    isPending: settings.isPending || results.some(({ query }) => query.isPending && query.isFetching),
    error: settings.error,
  };
}

function requireClient(client: ReturnType<typeof useDaemon>['client']) {
  if (!client) {throw new Error('Goddard daemon is disconnected');}
  return client;
}
