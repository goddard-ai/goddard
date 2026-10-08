import {
  ActivityIndicator,
  Text,
  TextInput,
  View,
  VirtualList,
} from '@octane-xplat/ui';

import { AppSymbol } from './app-symbol';
import { Sheet, SheetRow } from './sheet';
import { Radius } from '@/lib/theme-colors';
import type { useComposerPicker } from '@/lib/use-composer-picker';
import { useModelSheetListHeight } from '@/lib/use-model-sheet-list-height';
import { useTheme } from '@/lib/use-theme';
import type { BossPlan } from '@waku/client';
import type { ComposerAutocompleteRow } from '@waku/client/composer-autocomplete';

type PickerRow = ComposerAutocompleteRow | { kind: 'plan'; plan: BossPlan };

export function ComposerContextPicker({
  kind, visible, plans, onSelectPlan, query, onQueryChange, rows, hasResults, loading, error, onSelect, onDismiss, onRetry,
}: ReturnType<typeof useComposerPicker>['picker']) {
  const theme = useTheme();
  const listHeight = useModelSheetListHeight(visible);
  const commands = kind === 'command';
  const items: PickerRow[] = [...plans.map((plan) => ({ kind: 'plan' as const, plan })), ...rows];
  return (
    <Sheet visible={visible} onDismiss={onDismiss} scrollable={false} title={commands ? 'Commands' : 'Mention files and plans'}>
      <View style={{
        borderRadius: Radius.medium,
        flexDirection: 'row',
        alignItems: 'center',
        paddingLeft: 12,
        paddingRight: 8,
        marginHorizontal: 6,
        marginBottom: 8,
        backgroundColor: theme.inset,
      }}>
        <TextInput
          placeholder={commands ? 'Search commands…' : 'Search files and plans…'}
          style={{ flex: '1 1', fontSize: 16, minHeight: 44, paddingVertical: 10, color: theme.text }}
          value={query}
          onChange={onQueryChange}
        />
        <View style={{ width: 28, alignItems: 'center', justifyContent: 'center' }}>
          {loading && <ActivityIndicator busy />}
        </View>
      </View>
      <VirtualList
        items={items}
        keyExtractor={(row: PickerRow) => row.kind === 'plan' ? `plan:${row.plan.id}` : row.kind === 'command' ? `command:${row.command.name}` : `file:${row.file.path}`}
        style={{ height: listHeight }}
        renderItem={(item: PickerRow) => item.kind === 'plan' ? (
          <SheetRow label={`@${item.plan.idea}`} description={`Plan · ${item.plan.planFile}`} onPress={() => onSelectPlan(item.plan)} leading={<AppSymbol name={{ ios: 'safari', android: 'explore', web: 'explore' }} size={17} tintColor={theme.textTertiary} />} />
        ) : (
          <SheetRow
            label={item.kind === 'command' ? `/${item.command.name}` : item.file.path}
            description={item.kind === 'command'
              ? [item.command.scope.toLowerCase(), item.command.argument_hint, item.command.description].filter(Boolean).join(' · ')
              : item.file.is_dir ? 'Folder' : 'File'}
            leading={item.kind === 'file' ? (
              <AppSymbol
                name={item.file.is_dir
                  ? { ios: 'folder', android: 'folder', web: 'folder' }
                  : { ios: 'doc', android: 'description', web: 'description' }}
                size={17}
                tintColor={theme.textTertiary}
              />
            ) : undefined}
            onPress={() => onSelect(item)}
          />
        )}
        renderEmpty={() => (
          <View>
            {error ? (
              <>
                <Text accessibilityLiveRegion="polite" style={{ fontSize: 14, lineHeight: '18px', paddingHorizontal: 12, paddingVertical: 14, color: theme.danger }}>{error}</Text>
                <SheetRow label="Retry" onPress={onRetry} />
              </>
            ) : (
              <Text style={{ fontSize: 14, lineHeight: '18px', paddingHorizontal: 12, paddingVertical: 14, color: theme.textTertiary }}>
                {loading && !hasResults
                  ? commands ? 'Loading commands…' : 'Finding files…'
                  : commands ? 'No matching commands' : 'No matching files or plans'}
              </Text>
            )}
          </View>
        )}
      />
    </Sheet>
  );
}
