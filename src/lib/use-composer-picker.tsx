import type {
  AgentSession,
  Automation,
  BossDeliverable,
  BossEmployee,
  BossPersona,
  BossPlan,
  FileEntry,
  Project,
  ProviderKind,
  ReportedCommand,
  SlashCommand,
} from '@waku/client';

import { mergeComposerCommands, withManagedGoalCommand, type ComposerAutocompleteRow, type ComposerTriggerKind } from '@waku/client/composer-autocomplete';
import { useCallback, useEffect, useMemo, useRef, useState } from 'octane';
import { dismissKeyboard } from './keyboard';

import type { ComposerTextInputHandle } from '@/components/composer-text-input.types';
import { insertPlanMention } from './plans';
import { armDeliverable } from './deliverable-arm';
import { useAutomations, useBossState, useDaemonSettings, useTaskState } from './daemon-queries';
import {
  composerPickerSearchState,
  insertComposerItem,
  insertComposerMention,
  searchComposerRows,
  selectionAfterComposerEdit,
  type ComposerPickerResults,
  type ComposerSelection,
} from './composer-completion';

import { daemonKeys, discoverComposerCommands, listComposerFiles } from './daemon-api';
import { daemonQueryEntry } from './daemon-data';
import { displaySessionTitle, sessionHasStarted } from './session-presentation';
import { useDaemonQuery } from './daemon-hooks';
import { useDaemon } from './daemon-hooks';

const NO_COMMANDS: SlashCommand[] = [];
const NO_REPORTED: ReportedCommand[] = [];
const NO_FILES: FileEntry[] = [];

/** A non-file mention row in the @-picker. Navigable refs insert
 * `goddard://` links; boss operands insert the provider token the desktop's
 * ref atoms carry; a deliverable arms the composer's chip instead of
 * inserting text. */
export type ComposerPickerMention =
  | { kind: 'session'; id: string; label: string; planning: boolean }
  | { kind: 'employee'; employee: BossEmployee }
  | { kind: 'deliverable'; deliverable: BossDeliverable }
  | { kind: 'project'; project: Project }
  | { kind: 'persona'; persona: BossPersona }
  | { kind: 'automation'; automation: Automation };

/** The text a picked mention contributes to the draft. */
function mentionInsertion(mention: ComposerPickerMention): string {
  const escape = (name: string) => name.replace(/"/g, '\\"');
  switch (mention.kind) {
    case 'session':
      return `[@${mention.label}](goddard://task/${mention.id})`;
    case 'employee':
      return `[@${mention.employee.identity.name}](goddard://task/${mention.employee.sessionId})`;
    case 'project':
      return `[project "${escape(mention.project.name)}" (path: ${mention.project.path})]`;
    case 'persona':
      return `[persona "${escape(mention.persona.name)}" (persona_id: ${mention.persona.id})]`;
    case 'automation':
      return `[automation "${escape(mention.automation.name)}" (automation_id: ${mention.automation.id})]`;
    case 'deliverable':
      return '';
  }
}

/** Explicit + menu pickers. Typing / or @ never opens a popup, and hiding
 * the keyboard leaves an open picker available for browsing. */
export function useComposerPicker({ text, onChangeText, provider, root, reported = NO_REPORTED, contextKey, session, project: surfaceProject }: {
  text: string;
  onChangeText: (text: string) => void;
  provider: ProviderKind | null;
  root: string | null;
  reported?: ReportedCommand[];
  contextKey: string;
  /** The session being composed, when the picker serves a session composer —
   * drives same-project session mentions, self-exclusion, and boss-surface
   * refs. New-task passes no session. */
  session?: AgentSession | null;
  /** The surface's project when no session supplies one — new-task's
   * selection — for same-project session mentions. */
  project?: Project | null;
}) {
  const daemon = useDaemon();
  const settings = useDaemonSettings();
  const boss = useBossState();
  const taskState = useTaskState().data;
  const project = surfaceProject
    ?? taskState?.projects.find((item) => item.id === session?.project_id)
    ?? null;
  /** The boss mints a project for its chat and planning sessions — composing
   * inside it unlocks the boss-refs the desktop's `surface_boss_key` gates. */
  const bossSurface = Boolean(project && boss.data && project.id === boss.data.identity.id);
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

  const automations = useAutomations(Boolean(visible && kind === 'file' && bossSurface));

  /** The @-picker's non-file pool: same-project sessions everywhere, boss
   * refs inside the boss's project. Names match a partial `@query` loosely —
   * the desktop ranks these ahead of file matches. */
  const mentions = useMemo<ComposerPickerMention[]>(() => {
    if (kind !== 'file') {return [];}
    const needle = query.trim().toLowerCase();
    const matches = (label: string) => !needle || label.toLowerCase().includes(needle);
    const out: ComposerPickerMention[] = [];

    for (const candidate of taskState?.sessions ?? []) {
      if (candidate.id === session?.id
        || candidate.side_chat_of != null
        || candidate.archived_at != null
        || !sessionHasStarted(candidate)
        || (project && candidate.project_id !== project.id)) {
        continue;
      }

      const label = displaySessionTitle(candidate);
      if (matches(label)) {
        out.push({ kind: 'session', id: candidate.id, label, planning: candidate.planning != null });
      }
    }

    if (bossSurface && boss.data) {
      for (const employee of boss.data.employees) {
        if (matches(`${employee.identity.name} ${employee.jobTitle}`)) {
          out.push({ kind: 'employee', employee });
        }
      }

      if (session) {
        for (const deliverable of boss.data.deliverables.filter((item) => !item.archivedAt)) {
          if (matches(`${deliverable.name} ${deliverable.path}`)) {
            out.push({ kind: 'deliverable', deliverable });
          }
        }
      }

      for (const candidate of taskState?.projects ?? []) {
        if (candidate.temporary || candidate.kind || candidate.id === project?.id) {continue;}
        if (matches(candidate.name)) {out.push({ kind: 'project', project: candidate });}
      }

      for (const persona of boss.data.personas) {
        if (matches(persona.name)) {out.push({ kind: 'persona', persona });}
      }

      for (const automation of automations.data ?? []) {
        if (matches(automation.name)) {out.push({ kind: 'automation', automation });}
      }
    }

    return out.slice(0, 48);
  }, [automations.data, boss.data, bossSurface, kind, project, query, session, taskState?.projects, taskState?.sessions]);

  const applyInsertion = (replacement: { text: string; cursor: number }) => {
    const nextSelection = { start: replacement.cursor, end: replacement.cursor };
    setCaret({ text: replacement.text, selection: nextSelection });
    setForcedSelection(nextSelection);
    onChangeText(replacement.text);
    focusAfterDismiss.current = true;
    setVisible(false);
  };

  return {
    open(next: ComposerTriggerKind) {
      if (!enabled) {return;}
      // Native menu / keyboard dismissal can move the field's selection.
      // Keep the insertion point the user had when opening the picker —
      // read the live caret, since selection events don't fire on every
      // platform and a tap-move may never have been reported.
      insertionPoint.current = {
        text,
        selection: inputRef.current?.getSelection?.() ?? selection,
      };

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
      plans: kind === 'file' ? (boss.data?.planning ?? []).filter((plan) => `${plan.idea} ${plan.planFile}`.toLowerCase().includes(query.toLowerCase())).slice(0, 32) : [],
      mentions,
      onSelectPlan: (plan: BossPlan) => {
        const target = insertionPoint.current?.text === text ? insertionPoint.current.selection : selection;
        applyInsertion(insertPlanMention(text, target, plan));
      },
      onSelectMention: (mention: ComposerPickerMention) => {
        // A deliverable arms the composer's chip rather than inserting text —
        // the pick is a send-time operand, not prose.
        if (mention.kind === 'deliverable') {
          if (session) {
            armDeliverable({ deliverable: mention.deliverable, sessionId: session.id, previewOpen: false });
          }

          focusAfterDismiss.current = true;
          setVisible(false);
          return;
        }

        const target = insertionPoint.current?.text === text ? insertionPoint.current.selection : selection;
        applyInsertion(insertComposerMention(text, target, mentionInsertion(mention)));
      },
      hasResults: search.hasResults,
      loading: !error && (source.isFetching || (kind === 'command' && settings.isPending) || search.pending),
      error: error ? (error instanceof Error ? error.message : String(error)) : null,
      onSelect: (row: ComposerAutocompleteRow) => {
        const target = insertionPoint.current?.text === text ? insertionPoint.current.selection : selection;
        applyInsertion(insertComposerItem(text, target, row));
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
