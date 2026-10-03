import type { FileEntry, ProviderKind, ReportedCommand, SlashCommand } from '@waku/client';
import { mergeComposerCommands, withManagedGoalCommand, type ComposerAutocompleteRow, type ComposerTriggerKind } from '@waku/client/composer-autocomplete';
import { useCallback, useEffect, useMemo, useRef, useState } from 'octane';
import { dismissKeyboard } from './keyboard';

import type { ComposerTextInputHandle } from '@/components/composer-text-input.types';
import { useDaemonSettings } from './daemon-queries';
import { composerPickerSearchState, insertComposerItem, searchComposerRows, selectionAfterComposerEdit, type ComposerPickerResults, type ComposerSelection } from './composer-completion';
import { daemonKeys, discoverComposerCommands, listComposerFiles } from './daemon-api';
import { daemonQueryEntry } from './daemon-data';
import { useDaemonQuery } from './daemon-hooks';
import { useDaemon } from './daemon-hooks';

const NO_COMMANDS: SlashCommand[] = [];
const NO_REPORTED: ReportedCommand[] = [];
const NO_FILES: FileEntry[] = [];

/** Explicit + menu pickers. Typing / or @ never opens a popup, and hiding
 * the keyboard leaves an open picker available for browsing. */
export function useComposerPicker({ text, onChangeText, provider, root, reported = NO_REPORTED, contextKey }: {
  text: string;
  onChangeText: (text: string) => void;
  provider: ProviderKind | null;
  root: string | null;
  reported?: ReportedCommand[];
  contextKey: string;
}) {
  const daemon = useDaemon();
  const settings = useDaemonSettings();
  const inputRef = useRef<ComposerTextInputHandle | null>(null);
  const setInputRef = useCallback((input: ComposerTextInputHandle | null) => { inputRef.current = input; }, []);
  const [kind, setKind] = useState<ComposerTriggerKind | null>(null);
  const [visible, setVisible] = useState(false);
  const [query, setQuery] = useState('');
  const [caret, setCaret] = useState({ text, selection: { start: text.length, end: text.length } });
  const [forcedSelection, setForcedSelection] = useState<ComposerSelection>();
  const insertionPoint = useRef<{ text: string; selection: ComposerSelection } | null>(null);
  const focusAfterDismiss = useRef(false);
  const profileId = daemon.activeProfile?.id ?? 'disconnected';
  const binaryOverride = provider ? settings.data?.provider_binary_overrides?.[provider] ?? null : null;
  const enabled = daemon.phase === 'connected' && Boolean(daemon.client && provider && root);
  const commandOptions = {
    queryKey: daemonKeys.composerCommands(profileId, provider, root, binaryOverride),
    queryFn: () => discoverComposerCommands(daemon.client!, provider!, root!, binaryOverride),
  };

  const catalog = useDaemonQuery(commandOptions.queryKey, commandOptions.queryFn, { enabled: enabled && visible && Boolean(settings.data) && kind === 'command', staleTime: 60_000 });
  const discovered = catalog.data ?? NO_COMMANDS;
  const commands = useMemo(() => withManagedGoalCommand(provider!, mergeComposerCommands(discovered, reported), 'Goal'), [discovered, reported, provider]);
  const files = useDaemonQuery(
    daemonKeys.composerFiles(profileId, root),
    () => listComposerFiles(daemon.client!, root!),
    { enabled: enabled && visible && kind === 'file', staleTime: 30_000 },
  );

  const fileIndex = files.data ?? NO_FILES;
  const scope = JSON.stringify([profileId, contextKey, root, provider, binaryOverride, kind]);
  const [results, setResults] = useState<ComposerPickerResults | null>(null);
  useEffect(() => {
    if (!kind || !visible) {return;}
    let cancelled = false;
    void searchComposerRows({ kind, query, start: 0, end: 0 }, commands, fileIndex, () => cancelled).then((rows) => {
      if (rows && !cancelled) {setResults({ scope, query, commands, files: fileIndex, rows });}
    });

    return () => { cancelled = true; };
  }, [commands, fileIndex, scope, kind, query, visible]);

  useEffect(() => {
    setVisible(false);
    setQuery('');
    setForcedSelection(undefined);
    focusAfterDismiss.current = false;
    insertionPoint.current = null;
  }, [contextKey, profileId, provider, root]);

  const search = composerPickerSearchState(results, scope, query, commands, fileIndex);
  const source = kind === 'file' ? files : catalog;
  const error = !enabled ? new Error('Connect to a daemon and choose a project first')
    : source.error ?? (kind === 'command' ? settings.error : null);

  const selection = caret.text === text ? caret.selection : { start: text.length, end: text.length };

  return {
    open(next: ComposerTriggerKind) {
      if (!enabled) {return;}
      // Native menu / keyboard dismissal can move the field's selection.
      // Keep the insertion point the user had when opening the picker.
      insertionPoint.current = { text, selection };
      dismissKeyboard();
      setQuery('');
      setKind(next);
      setVisible(true);
    },
    async getCommands() {
      if (!enabled) {throw new Error('Connect to a daemon and choose a project first');}
      // Sending a chosen or typed command also waits for its catalog, so
      // project templates cannot be bypassed by a quick send.
      const readySettings = settings.data ?? (await settings.refetch()) ?? undefined;
      if (!readySettings) {throw settings.error ?? new Error('Could not load daemon settings');}
      const override = readySettings.provider_binary_overrides?.[provider!] ?? null;
      const entry = daemonQueryEntry(
        daemonKeys.composerCommands(profileId, provider, root, override),
        () => discoverComposerCommands(daemon.client!, provider!, root!, override),
        60_000,
      );

      const discoveredCommands = (await entry.refetch())!;
      return withManagedGoalCommand(provider!, mergeComposerCommands(discoveredCommands, reported), 'Goal');
    },
    inputProps: {
      inputRef: setInputRef,
      onChangeText: (value: string) => {
        setForcedSelection(undefined);
        setCaret({ text: value, selection: selectionAfterComposerEdit(text, value, selection) });
        onChangeText(value);
      },
      onSelectionChange: (event: { nativeEvent: { selection: ComposerSelection } }) => {
        const next = event.nativeEvent.selection;
        if (forcedSelection && (next.start !== forcedSelection.start || next.end !== forcedSelection.end)) {return;}
        setForcedSelection(undefined);
        setCaret({ text, selection: next });
      },
      selection: forcedSelection,
    },
    picker: {
      kind,
      visible,
      query,
      onQueryChange: setQuery,
      rows: search.rows,
      hasResults: search.hasResults,
      loading: !error && (source.isFetching || (kind === 'command' && settings.isPending) || search.pending),
      error: error ? (error instanceof Error ? error.message : String(error)) : null,
      onSelect: (row: ComposerAutocompleteRow) => {
        const target = insertionPoint.current?.text === text ? insertionPoint.current.selection : selection;
        const replacement = insertComposerItem(text, target, row);
        const nextSelection = { start: replacement.cursor, end: replacement.cursor };
        setCaret({ text: replacement.text, selection: nextSelection });
        setForcedSelection(nextSelection);
        onChangeText(replacement.text);
        focusAfterDismiss.current = true;
        setVisible(false);
      },
      onDismiss: () => {
        setVisible(false);
        if (focusAfterDismiss.current) {
          focusAfterDismiss.current = false;
          inputRef.current?.focus();
        }
      },
      onRetry: () => {
        if (!enabled) {return;}
        if (kind === 'command' && !settings.data) {void settings.refetch();}
        else {void source.refetch();}
      },
    },
  };
}
