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
import type { ComposerAutocompleteRow } from '@waku/client/composer-autocomplete';

export function ComposerContextPicker({
  kind, visible, query, onQueryChange, rows, hasResults, loading, error, onSelect, onDismiss, onRetry,
}: ReturnType<typeof useComposerPicker>['picker']) {
  const theme = useTheme();
  const listHeight = useModelSheetListHeight(visible);
  const commands = kind === 'command';
  return (
    <Sheet visible={visible} onDismiss={onDismiss} scrollable={false} title={commands ? 'Commands' : 'Mention files'}>
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
          placeholder={commands ? 'Search commands…' : 'Search project files…'}
          style={{ flex: 1, fontSize: 16, minHeight: 44, paddingVertical: 10, color: theme.text }}
          value={query}
          onChange={onQueryChange}
        />
        <View style={{ width: 28, alignItems: 'center', justifyContent: 'center' }}>
          {loading && <ActivityIndicator busy />}
        </View>
      </View>
      <VirtualList
        items={rows}
        keyExtractor={(row: ComposerAutocompleteRow) => row.kind === 'command' ? `command:${row.command.name}` : `file:${row.file.path}`}
        style={{ height: listHeight }}
        renderItem={(item: ComposerAutocompleteRow) => (
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
                  : commands ? 'No matching commands' : 'No matching files'}
              </Text>
            )}
          </View>
        )}
      />
    </Sheet>
  );
}
