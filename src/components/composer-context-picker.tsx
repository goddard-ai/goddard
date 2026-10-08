import {
  ActivityIndicator,
  Text,
  TextInput,
  View,
  VirtualList,
} from '@octane-xplat/ui';

import { AppSymbol } from './app-symbol';
import { BossAvatar } from './boss-roster';
import { Sheet, SheetRow } from './sheet';
import { Radius } from '@/lib/theme-colors';
import type { ComposerPickerMention, useComposerPicker } from '@/lib/use-composer-picker';
import { useModelSheetListHeight } from '@/lib/use-model-sheet-list-height';
import { useTheme } from '@/lib/use-theme';
import { deliverableFileName } from '@/lib/deliverables';
import type { BossPlan } from '@waku/client';
import type { ComposerAutocompleteRow } from '@waku/client/composer-autocomplete';

type PickerRow =
  | ComposerAutocompleteRow
  | { kind: 'plan'; plan: BossPlan }
  | { kind: 'mention'; mention: ComposerPickerMention };

export function ComposerContextPicker({
  kind, visible, plans, mentions, onSelectPlan, onSelectMention, query, onQueryChange, rows, hasResults, loading, error, onSelect, onDismiss, onRetry,
}: ReturnType<typeof useComposerPicker>['picker']) {
  const theme = useTheme();
  const listHeight = useModelSheetListHeight(visible);
  const commands = kind === 'command';
  const items: PickerRow[] = [
    ...plans.map((plan) => ({ kind: 'plan' as const, plan })),
    ...mentions.map((mention) => ({ kind: 'mention' as const, mention })),
    ...rows,
  ];

  return (
    <Sheet visible={visible} onDismiss={onDismiss} scrollable={false} title={commands ? 'Commands' : 'Mention tasks, plans, and files'}>
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
          placeholder={commands ? 'Search commands…' : 'Search mentions and files…'}
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
        keyExtractor={(row: PickerRow) => {
          if (row.kind === 'plan') {return `plan:${row.plan.id}`;}
          if (row.kind === 'mention') {return mentionKey(row.mention);}
          return row.kind === 'command' ? `command:${row.command.name}` : `file:${row.file.path}`;
        }}
        style={{ height: listHeight }}
        renderItem={(item: PickerRow) => item.kind === 'plan' ? (
          <SheetRow label={`@${item.plan.idea}`} description={`Plan · ${item.plan.planFile}`} onPress={() => onSelectPlan(item.plan)} leading={<AppSymbol name={{ ios: 'safari', android: 'explore', web: 'explore' }} size={17} tintColor={theme.textTertiary} />} />
        ) : item.kind === 'mention' ? (
          <MentionRow mention={item.mention} onSelect={onSelectMention} />
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
                  ? commands ? 'Loading commands…' : 'Finding mentions and files…'
                  : commands ? 'No matching commands' : 'No matching mentions or files'}
              </Text>
            )}
          </View>
        )}
      />
    </Sheet>
  );
}

function mentionKey(mention: ComposerPickerMention): string {
  switch (mention.kind) {
    case 'session': return `session:${mention.id}`;
    case 'employee': return `employee:${mention.employee.sessionId}`;
    case 'deliverable': return `deliverable:${mention.deliverable.id}`;
    case 'project': return `project:${mention.project.id}`;
    case 'persona': return `persona:${mention.persona.id}`;
    case 'automation': return `automation:${mention.automation.id}`;
  }
}

function MentionRow({
  mention,
  onSelect,
}: {
  mention: ComposerPickerMention;
  onSelect: (mention: ComposerPickerMention) => void;
}) {
  const theme = useTheme();
  const icon = (name: Parameters<typeof AppSymbol>[0]['name']) => (
    <AppSymbol name={name} size={17} tintColor={theme.textTertiary} />
  );

  switch (mention.kind) {
    case 'session':
      return (
        <SheetRow
          description={mention.planning ? 'Plan' : 'Task'}
          label={`@${mention.label}`}
          leading={icon(mention.planning
            ? { ios: 'safari', android: 'explore', web: 'explore' }
            : { ios: 'bubble.left', android: 'chat_bubble_outline', web: 'chat_bubble_outline' })}
          onPress={() => onSelect(mention)}
        />
      );
    case 'employee':
      return (
        <SheetRow
          description={`Employee${mention.employee.jobTitle ? ` · ${mention.employee.jobTitle}` : ''}`}
          label={`@${mention.employee.identity.name}`}
          leading={<BossAvatar identity={mention.employee.identity} size={20} />}
          onPress={() => onSelect(mention)}
        />
      );
    case 'deliverable':
      return (
        <SheetRow
          description={`Deliverable · ${deliverableFileName(mention.deliverable)}`}
          label={`@${mention.deliverable.name}`}
          leading={icon({ ios: 'shippingbox', android: 'inventory_2', web: 'inventory_2' })}
          onPress={() => onSelect(mention)}
        />
      );
    case 'project':
      return (
        <SheetRow
          description={`Project · ${mention.project.path}`}
          label={`@${mention.project.name}`}
          leading={icon({ ios: 'folder', android: 'folder', web: 'folder' })}
          onPress={() => onSelect(mention)}
        />
      );
    case 'persona':
      return (
        <SheetRow
          description="Persona"
          label={`@${mention.persona.name}`}
          leading={icon({ ios: 'person.crop.square', android: 'badge', web: 'badge' })}
          onPress={() => onSelect(mention)}
        />
      );
    case 'automation':
      return (
        <SheetRow
          description="Automation"
          label={`@${mention.automation.name}`}
          leading={icon({ ios: 'clock', android: 'schedule', web: 'schedule' })}
          onPress={() => onSelect(mention)}
        />
      );
  }
}
