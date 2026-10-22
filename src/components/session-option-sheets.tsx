import type { ProviderKind, ProviderModel, RuntimeMode } from '@waku/client';
import { useEffect, useMemo, useState } from 'octane';
import { Spinner, Pressable, Text, TextInput, View, VirtualList } from '@octane-xplat/ui';

import { AppSymbol } from './app-symbol';
import { ProviderIcon } from './provider-icon';
import { Sheet, SheetRow } from './sheet';
import { selectionHaptic } from '@/lib/haptics';
import { NativeTint } from '@/lib/theme-platform';
import { Radius } from '@/lib/theme-colors';
import { useAllProviderModels, useProviderModels } from '@/lib/daemon-queries';
import { useModelSheetListHeight } from '@/lib/use-model-sheet-list-height';
import { useTheme } from '@/lib/use-theme';
import {
  resolveModelTraitSelection,
  resolveServiceTier,
  type ModelTraitSelection,
} from '@/lib/model-traits';

import { providerLabel, runtimeModeLabel } from '@/lib/session-presentation';

export interface ModelSelection {
  model: string | null;
  reasoningEffort: string | null;
}

export function modelDisplayName(
  models: ProviderModel[] | undefined,
  model: string | null,
): string {
  if (!model) {return 'Default model';}
  return models?.find((item) => item.id === model)?.name ?? model;
}

const noteStyle = { fontSize: 14, lineHeight: '18px', paddingLeft: 12, paddingRight: 12, paddingTop: 10, paddingBottom: 10 };
const sectionTitleStyle = {
  fontSize: 13,
  fontWeight: '600',
  letterSpacing: 0.4,
  marginBottom: 6,
  marginLeft: 12, marginRight: 12,
  marginTop: 14,
};

/** Current-provider model picker opened from the task header. */
export function ModelSheet({
  visible,
  onDismiss,
  provider,
  model,
  onApply,
}: {
  visible: boolean;
  onDismiss: () => void;
  provider: ProviderKind;
  model: string | null;
  onApply: (selection: ModelSelection) => void;
}) {
  const theme = useTheme();
  const listHeight = useModelSheetListHeight(visible);
  const probe = useProviderModels(provider);
  const [search, setSearch] = useState('');
  const models = probe.data?.models ?? [];
  const defaultModel = models.find((item) => item.is_default) ?? models[0];
  const items = useMemo(
    () => filterModels(probe.data?.models, search),
    [probe.data?.models, search],
  );

  useEffect(() => {
    if (visible) {setSearch('');}
  }, [provider, visible]);

  function pickModel(next: ProviderModel) {
    selectionHaptic();
    onApply({
      model: next.id,
      reasoningEffort: next.default_reasoning_effort ?? null,
    });

    onDismiss();
  }

  return (
    <Sheet
      onDismiss={onDismiss}
      scrollable={false}
      visible={visible}>
      <ModelSearchField onChangeText={setSearch} value={search} />
      {probe.isPending ? (
        <View style={{ alignItems: 'center', justifyContent: 'center', paddingTop: 40, paddingBottom: 40 }}>
          <Spinner busy />
        </View>
      ) : probe.error ? (
        <Text style={{ ...noteStyle, color: theme.danger }}>
          {probe.error instanceof Error ? probe.error.message : String(probe.error)}
        </Text>
      ) : (
        <VirtualList
          items={items}
          keyExtractor={(item: ProviderModel) => item.id}
          renderItem={(item: ProviderModel) => (
            <SheetRow
              description={item.sub_provider ?? undefined}
              label={item.name}
              onPress={() => pickModel(item)}
              selected={model === item.id || (!model && defaultModel?.id === item.id)}
            />
          )}
          style={{ height: listHeight }}
          renderEmpty={() => (
            <Text style={{ ...noteStyle, color: theme.textTertiary }}>
              {search.trim()
                ? 'No models match your search.'
                : 'This agent doesn’t expose a model list; it will use its own default.'}
            </Text>
          )}
        />
      )}
    </Sheet>
  );
}

/** All model-advertised options in one sheet. Unlike single-choice pickers,
 * it stays open after a choice so effort, tier, and context can be configured
 * together. */
export function ModelTraitsSheet({
  visible,
  onDismiss,
  model,
  selection,
  onApply,
}: {
  visible: boolean;
  onDismiss: () => void;
  model: ProviderModel;
  selection: ModelTraitSelection;
  onApply: (changes: Partial<ModelTraitSelection>) => void;
}) {
  const theme = useTheme();
  const resolved = resolveModelTraitSelection(model, selection);

  function pick(changes: Partial<ModelTraitSelection>) {
    selectionHaptic();
    onApply(changes);
  }

  return (
    <Sheet onDismiss={onDismiss} visible={visible}>
      {model.reasoning_efforts.length ? (
        <>
          <Text style={{ ...sectionTitleStyle, color: theme.textSecondary }}>
            REASONING EFFORT
          </Text>
          {model.reasoning_efforts.map((option) => (
            <SheetRow
              description={optionDescription(
                option.description,
                model.default_reasoning_effort === option.id,
              )}
              label={option.label}
              onPress={() => pick({ reasoningEffort: option.id })}
              selected={resolved.reasoningEffort === option.id}
            />
          ))}
        </>
      ) : null}
      <ServiceTierOptions
        model={model}
        onApply={(tier) => pick({ serviceTier: tier })}
        serviceTier={selection.serviceTier}
      />
      {model.context_windows.length ? (
        <>
          <Text style={{ ...sectionTitleStyle, color: theme.textSecondary }}>
            CONTEXT WINDOW
          </Text>
          {model.context_windows.map((option) => (
            <SheetRow
              description={optionDescription(
                option.description,
                model.default_context_window === option.id,
              )}
              label={option.label}
              onPress={() => pick({ contextWindow: option.id })}
              selected={resolved.contextWindow === option.id}
            />
          ))}
        </>
      ) : null}
    </Sheet>
  );
}

/** Shared by the new-task and ongoing-task composers. Keep the provider's
 * concrete tier IDs, including `priority` when it advertises Fast that way. */
function ServiceTierOptions({
  model,
  serviceTier,
  onApply,
}: {
  model: ProviderModel;
  serviceTier: string | null;
  onApply: (tier: string) => void;
}) {
  const theme = useTheme();
  if (!model.service_tiers.length) {return null;}
  const selected = resolveServiceTier(model, serviceTier);

  return (
    <>
      <Text style={{ ...sectionTitleStyle, color: theme.textSecondary }}>
        SERVICE TIER
      </Text>
      <SheetRow
        description={(model.default_service_tier ?? 'default') === 'default'
          ? 'Default'
          : undefined}
        label="Standard"
        onPress={() => onApply('default')}
        selected={selected === 'default'}
      />
      {model.service_tiers.map((option) => (
        <SheetRow
          description={optionDescription(
            option.description,
            model.default_service_tier === option.id,
          )}
          label={option.label}
          onPress={() => onApply(option.id)}
          selected={selected === option.id}
        />
      ))}
    </>
  );
}

export interface ProviderModelSelection extends ModelTraitSelection {
  provider: ProviderKind;
  model: string | null;
}

/**
 * Two-screen cross-provider model picker: opening lands on the current
 * provider's models with a search filter over a virtualized list; the back
 * row (named after the provider) slides across to the providers screen, and
 * choosing a provider slides back into that provider's models.
 *
 * TODO(port): the reference animated a 260ms page slide; xplat swaps pages
 * instantly — revisit if the slide matters.
 */
export function ModelPickerSheet({
  visible,
  onDismiss,
  providers,
  provider,
  model,
  onApply,
}: {
  visible: boolean;
  onDismiss: () => void;
  providers: ProviderKind[];
  provider: ProviderKind | null;
  model: string | null;
  onApply: (selection: ProviderModelSelection) => void;
}) {
  const theme = useTheme();
  const listHeight = useModelSheetListHeight(visible);
  const catalog = useAllProviderModels(visible ? providers : []);
  const [browsing, setBrowsing] = useState<ProviderKind | null>(provider);
  const [search, setSearch] = useState('');
  const [page, setPage] = useState<0 | 1>(provider ? 1 : 0);

  useEffect(() => {
    if (!visible) {return;}
    const initial = provider ?? providers[0] ?? null;
    setBrowsing(initial);
    setSearch('');
    setPage(initial ? 1 : 0);
  }, [provider, providers, visible]);

  const entry = catalog.find((item) => item.id === browsing);
  const preferredModelId = entry?.models.find((item) => item.is_default)?.id
    ?? entry?.models[0]?.id;

  const items = useMemo(
    () => filterModels(entry?.models, search),
    [entry?.models, search],
  );

  function pickModel(next: ProviderModel) {
    if (!browsing) {return;}
    selectionHaptic();
    onApply({
      provider: browsing,
      model: next.id,
      reasoningEffort: next.default_reasoning_effort ?? null,
      serviceTier: next.default_service_tier ?? null,
      contextWindow: next.default_context_window ?? null,
    });

    onDismiss();
  }

  return (
    <Sheet onDismiss={onDismiss} scrollable={false} visible={visible}>
      {page === 0 ? (
        <VirtualList
          items={providers}
          keyExtractor={(id: ProviderKind) => id}
          renderItem={(id: ProviderKind) => (
            <SheetRow
              label={providerLabel(id)}
              leading={<ProviderIcon provider={id} size={20} />}
              onPress={() => {
                selectionHaptic();
                setBrowsing(id);
                setSearch('');
                setPage(1);
              }}
              selected={id === provider}
            />
          )}
          style={{ height: listHeight }}
          renderEmpty={() => (
            <Text style={{ ...noteStyle, color: theme.textTertiary }}>
              No agents are installed on this daemon host.
            </Text>
          )}
        />
      ) : (
        <View>
          <Pressable
            accessibilityHint="Shows all providers"
            accessibilityLabel={browsing ? providerLabel(browsing) : 'Provider'}
            accessibilityRole="button"
            onPress={() => setPage(0)}
            style={{
              alignItems: 'center',
              flexDirection: 'row',
              gap: 5,
              minHeight: 40,
              paddingLeft: 10,
              paddingRight: 10,
            }}
          >
            <AppSymbol
              name={{ ios: 'chevron.left', android: 'arrow_back', web: 'arrow_back' }}
              size={14}
              tintColor={NativeTint as string}
            />
            <Text style={{ fontSize: 16, fontWeight: '600', color: NativeTint as string }}>
              {browsing ? providerLabel(browsing) : 'Provider'}
            </Text>
          </Pressable>
          <ModelSearchField onChangeText={setSearch} value={search} />
          {entry?.isPending ? (
            <View style={{ alignItems: 'center', justifyContent: 'center', paddingTop: 40, paddingBottom: 40, height: listHeight }}>
              <Spinner busy />
            </View>
          ) : (
            <VirtualList
              items={items}
              keyExtractor={(item: ProviderModel) => item.id}
              renderItem={(item: ProviderModel) => (
                <SheetRow
                  description={item.sub_provider ?? undefined}
                  label={item.name}
                  onPress={() => pickModel(item)}
                  selected={provider === browsing &&
                    (model === item.id || (!model && item.id === preferredModelId))}
                />
              )}
              style={{ height: listHeight }}
              renderEmpty={() => (
                <Text style={{ ...noteStyle, color: theme.textTertiary }}>
                  {search.trim()
                    ? 'No models match your search.'
                    : 'This agent doesn’t expose a model list; it will use its own default.'}
                </Text>
              )}
            />
          )}
        </View>
      )}
    </Sheet>
  );
}

function filterModels(models: ProviderModel[] | undefined, search: string): ProviderModel[] {
  if (!models) {return [];}
  const query = search.trim().toLocaleLowerCase();
  if (!query) {return models;}
  return models.filter((item) => (
    item.name.toLocaleLowerCase().includes(query) ||
      item.id.toLocaleLowerCase().includes(query) ||
      item.sub_provider?.toLocaleLowerCase().includes(query)
  ));
}

function ModelSearchField({
  value,
  onChangeText,
}: {
  value: string;
  onChangeText: (value: string) => void;
}) {
  const theme = useTheme();
  return (
    <View style={{
      alignItems: 'center',
      borderRadius: Radius.medium,
      flexDirection: 'row',
      gap: 7,
      marginBottom: 8,
      marginLeft: 4,
      marginRight: 4,
      minHeight: 38,
      paddingLeft: 10,
      paddingRight: 10,
      backgroundColor: theme.overlayStrong,
    }}>
      <AppSymbol
        name={{ ios: 'magnifyingglass', android: 'search', web: 'search' }}
        size={14}
        tintColor={theme.textTertiary}
      />
      <TextInput
        placeholder="Search models"
        style={{ flex: '1 1', fontSize: 16, paddingTop: 7, paddingBottom: 7, color: theme.text }}
        value={value}
        onChange={onChangeText}
      />
      {value.length > 0 && (
        <Pressable
          accessibilityLabel="Clear search"
          accessibilityRole="button"
          onPress={() => onChangeText('')}
        >
          <AppSymbol
            name={{ ios: 'xmark.circle.fill', android: 'cancel', web: 'cancel' }}
            size={15}
            tintColor={theme.textTertiary}
          />
        </Pressable>
      )}
    </View>
  );
}

const ACCESS_MODES: Array<{ id: RuntimeMode; description: string }> = [
  { id: 'ask', description: 'Approve every command and file edit.' },
  { id: 'autoAcceptEdits', description: 'Edits apply automatically; commands still ask.' },
  { id: 'auto', description: 'Works autonomously inside the project.' },
  { id: 'fullAccess', description: 'No approval prompts. The agent acts freely.' },
];

/** Access-mode picker mirroring the desktop composer's access control. */
export function AccessSheet({
  visible,
  onDismiss,
  mode,
  onApply,
}: {
  visible: boolean;
  onDismiss: () => void;
  mode: RuntimeMode;
  onApply: (mode: RuntimeMode) => void;
}) {
  return (
    <Sheet onDismiss={onDismiss} title="Agent access" visible={visible}>
      {ACCESS_MODES.map((item) => (
        <SheetRow
          description={item.description}
          label={runtimeModeLabel(item.id)}
          onPress={() => {
            selectionHaptic();
            onApply(item.id);
            onDismiss();
          }}
          selected={mode === item.id}
        />
      ))}
    </Sheet>
  );
}

function optionDescription(description: string | null | undefined, isDefault: boolean) {
  if (description && isDefault) {return `Default · ${description}`;}
  return description ?? (isDefault ? 'Default' : undefined);
}
