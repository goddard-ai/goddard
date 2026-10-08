import type {
  AgentSession,
  BossDeliverable,
  BossEmployee,
  BossIdentity,
  MessageAttachment,
  PendingPermission,
  PendingUserInput,
  UserInputAnswer,
} from '@waku/client';

import { openBossSession } from '@waku/client/boss';

import {
  annotationBubbleContent,
  annotationPromptPrefix,
  attachmentPromptToken,
  isAgentQueuedMessage,
  sessionAcceptsImmediateSteer,
} from '@waku/client';

import { atomVisibleText } from '@waku/client/transcript-presentation';
import { parseRenameSubmission } from '@waku/client/composer-autocomplete';
import { useEffect, useRef, useState, type ReactNode } from 'octane';
import {
  ActivityIndicator,
  Pressable,
  ScrollView,
  Text,
  TextArea,
  pushRoute,
  useSafeAreaInsets,
  View,
} from '@octane-xplat/ui';

import { AppSymbol } from './app-symbol';
import { BossAvatar } from './boss-roster';
import { sessionReferenceAttachment } from '@/lib/boss';
import { AttachmentTile } from './attachment-tile';
import { ComposerAccessMenu } from './composer-access-menu';
import { ComposerContextPicker } from './composer-context-picker';
import { useComposerLocalCommands } from './composer-command-sheets';
import {
  ComposerAddMenu,
  type ComposerAttachmentSource,
} from './composer-add-menu';

import { ComposerTextInput } from './composer-text-input';
import type { ComposerTextInputProps } from './composer-text-input.types';
import { deliverableAttachment } from '@/lib/deliverables';
import {
  clearDeliverableArm,
  showArmedDeliverablePreview,
} from '@/lib/deliverable-arm';

import { GlassSurface, liquidGlass } from './glass-surface';
import { ModelTraitsSheet } from './session-option-sheets';
import { Haptics } from '@/lib/haptics';
import { pickAttachments } from '@/lib/pick-attachment';
import { MonoFont, NativeTint } from '@/lib/theme-platform';
import { Radius } from '@/lib/theme-colors';
import { useProviderModels, useTaskState } from '@/lib/daemon-queries';
import { useComposerPicker } from '@/lib/use-composer-picker';
import { useComposerKeyboardPadding } from '@/lib/use-keyboard-padding';
import { useSyncedComposerDraft } from '@/lib/use-synced-composer-draft';
import { useTheme } from '@/lib/use-theme';
import {
  importLocalAttachment,
  type LocalAttachmentFile,
} from '@/lib/attachments';

import { useDaemon } from '@/lib/daemon-hooks';
import { landWorkspace } from '@/lib/daemon-api';
import { sessionBusy, sessionCwd } from '@/lib/mobile-runtime';
import { composerProviderPrompt } from '@/lib/composer-completion';
import { modelHasConfigurableTraits } from '@/lib/model-traits';
import { useRuntime } from '@/lib/use-runtime';
import { isDaemonDisconnectError } from '@/lib/runtime-errors';

const hairline = 0.5;

/**
 * The composer surface shared by the session screen and the new-task screen:
 * a tall rounded card holding the input with an icon toolbar underneath —
 * option toggles on the left, meters and the send button on the right.
 */
export function ComposerCard({
  beforeInput,
  left,
  right,
  ...inputProps
}: ComposerTextInputProps & {
  beforeInput?: ReactNode;
  left?: ReactNode;
  right?: ReactNode;
}) {
  const theme = useTheme();
  return (
    <GlassSurface
      fallbackColor={theme.composer}
      interactive
      style={[
        {
          borderRadius: 26,
          paddingBottom: 8,
          paddingHorizontal: 10,
          paddingTop: 6,
        },
        !liquidGlass && {
          borderColor: theme.border,
          borderWidth: hairline,
        },
      ]}>
      {beforeInput}
      <ComposerTextInput
        multiline
        style={{
          fontSize: 17,
          lineHeight: '21px',
          maxHeight: 120,
          minHeight: 42,
          paddingHorizontal: 6,
          paddingVertical: 8,
          color: theme.text,
        }}
        {...inputProps}
      />
      <View style={{ alignItems: 'center', flexDirection: 'row', marginTop: 2 }}>
        <View style={{ alignItems: 'center', flexDirection: 'row', gap: 2 }}>{left}</View>
        <View style={{ flex: '1 1' }} />
        <View style={{ alignItems: 'center', flexDirection: 'row', gap: 2 }}>{right}</View>
      </View>
    </GlassSurface>
  );
}

export function ComposerIconButton({
  icon,
  label,
  onPress,
  active = false,
  disabled = false,
}: {
  icon: Parameters<typeof AppSymbol>[0]['name'];
  label: string;
  onPress: () => void;
  active?: boolean;
  disabled?: boolean;
}) {
  const theme = useTheme();
  return (
    <Pressable
      accessibilityLabel={label}
      accessibilityRole="button"
      accessibilityState={{ disabled, selected: active }}
      disabled={disabled}
      onPress={onPress}
      style={{
        alignItems: 'center',
        borderRadius: Radius.pill,
        height: 36,
        justifyContent: 'center',
        width: 36,
        backgroundColor: active ? theme.overlayStrong : 'transparent',
        opacity: disabled ? 0.35 : 1,
      }}>
      <AppSymbol name={icon} size={19} tintColor={(active ? NativeTint : theme.textSecondary) as string} />
    </Pressable>
  );
}

export function SendButton({
  onPress,
  disabled,
  busy = false,
  steering = false,
  queueing = false,
  label,
}: {
  onPress: () => void;
  disabled: boolean;
  busy?: boolean;
  steering?: boolean;
  queueing?: boolean;
  label: string;
}) {
  const theme = useTheme();
  return (
    <Pressable
      accessibilityLabel={label}
      accessibilityRole="button"
      disabled={disabled}
      onPress={onPress}
      style={{
        alignItems: 'center',
        borderRadius: Radius.pill,
        height: 36,
        justifyContent: 'center',
        marginLeft: 4,
        width: 36,
        backgroundColor: disabled
          ? theme.surfaceMuted
          : steering ? NativeTint : theme.inverse,
        opacity: busy ? 0.6 : 1,
      }}>
      {busy ? (
        <ActivityIndicator busy />
      ) : (
        <AppSymbol
          name={queueing
            ? { ios: 'text.append', android: 'playlist_add', web: 'playlist_add' }
            : { ios: 'arrow.up', android: 'arrow_upward', web: 'arrow_upward' }}
          size={17}
          tintColor={(disabled ? theme.textTertiary : steering ? '#ffffff' : theme.onInverse) as string}
        />
      )}
    </Pressable>
  );
}

export function MobileComposer({
  session,
  boss,
  deliverable,
  onSubmitted,
  onPermissionRespond,
}: {
  session: AgentSession;
  /** Set when the viewed session is a boss employee: every submission is
   * routed to the boss chat with a session-reference attachment, then the
   * boss chat comes on screen — the same supervision command the desktop
   * arms while an employee is open. */
  boss?: { identity: BossIdentity; employee: BossEmployee } | null;
  /** Set while a deliverable click armed this chat's composer — the file
   * rides the next submission as an `@path` attachment, the same command
   * context the desktop's `command_deliverable` carries. */
  deliverable?: BossDeliverable | null;
  /** Fires as the submission begins — before the runtime round-trip — so the
   * transcript can pin to the tail the moment the message lands. */
  onSubmitted?: () => void;
  onPermissionRespond?: (optionId: string) => Promise<void>;
}) {
  const theme = useTheme();
  const insets = useSafeAreaInsets();
  const shellPadding = useComposerKeyboardPadding(insets.bottom);
  const daemon = useDaemon();
  const runtime = useRuntime();
  const [draft, setDraft] = useState('');
  const [attachments, setAttachments] = useState<MessageAttachment[]>([]);
  const [importingAttachments, setImportingAttachments] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [localError, setLocalError] = useState<string | null>(null);
  const [traitsSheetOpen, setTraitsSheetOpen] = useState(false);
  const modelProbe = useProviderModels(session.provider);
  const models = modelProbe.data?.models ?? [];
  const activeModel = session.model
    ? models.find((item) => item.id === session.model)
    : models.find((item) => item.is_default) ?? models[0];

  const busy = sessionBusy(session);
  const liveRuntime = runtime.runtimes[session.id];
  const canSteer = sessionAcceptsImmediateSteer(session) && Boolean(liveRuntime?.supportsSteer);
  const permission = runtime.permissions[session.id];
  const userInput = runtime.userInputs[session.id];
  const runtimeError = runtime.errors[session.id];
  const connected = daemon.phase === 'connected';
  const visibleLocalError = connected && isDaemonDisconnectError(localError) ? null : localError;
  const visibleRuntimeError = connected && isDaemonDisconnectError(runtimeError)
    ? null
    : runtimeError;

  const visibleError = visibleLocalError || visibleRuntimeError;
  const queued = session.queued_messages ?? [];
  const taskState = useTaskState();
  const project = taskState.data?.projects.find((item) => item.id === session.project_id);

  useEffect(() => setLocalError(null), [session.id]);
  useEffect(() => {
    if (connected && isDaemonDisconnectError(localError)) {setLocalError(null);}
  }, [connected, localError]);

  // Cross-device draft, persisted on the daemon like the desktop composer:
  // hydrate when this surface becomes active, save only local edits, and
  // clear on send. A daemon-loaded value must never be echoed back as an edit.
  const draftSync = useSyncedComposerDraft({
    target: { type: 'session', sessionId: session.id },
    text: draft,
    attachments,
    onHydrate: (synchronized) => {
      setDraft(synchronized.text);
      setAttachments(synchronized.attachments);
    },
    flushOnUnmount: true,
  });

  const contextPicker = useComposerPicker({
    text: draft,
    onChangeText: (value) => {
      draftSync.markEdited();
      setDraft(value);
    },
    provider: session.provider,
    root: project ? sessionCwd(session, project) : null,
    reported: session.available_commands,
    contextKey: session.id,
    session,
  });

  const localCommands = useComposerLocalCommands({
    provider: session.provider,
    model: activeModel,
    serviceTier: session.service_tier,
    runtimeMode: session.runtime_mode,
    goal: session.thread_goal,
    contextKey: session.id,
    onServiceTier: (serviceTier) => runtime.updateSessionOptions(session.id, { serviceTier }),
    onGoal: (operation, managed) => runtime.sendGoalOperation(session, operation, managed),
    onRename: async (title) => {
      if (title === null) {
        await runtime.grantAgentRename(session.id);
        return;
      }

      await runtime.renameSession(session.id, title);
    },
    onLand: async () => {
      if (!daemon.client || !project) {throw new Error('Select a task to see its Git state');}
      const outcome = await landWorkspace(
        daemon.client,
        sessionCwd(session, project),
        session.workspace?.kind === 'worktree' ? session.workspace.baseBranch ?? null : null,
      );

      if ('landed' in outcome) {
        await Haptics.notificationAsync('success');
        return;
      }

      // Nothing to send: the requested end state already holds.
      if ('alreadyLanded' in outcome) {
        return;
      }

      // Mobile has no conflict modal; the banner carries the way out.
      const kind = outcome.conflict.in_progress === 'rebase' ? 'rebase' : 'merge';
      throw new Error(
        `Landing onto ${outcome.conflict.base} stopped on ${kind} conflicts. Resolve them in the worktree, then run /land again.`,
      );
    },
    onClear: () => {
      draftSync.markEdited();
      setDraft('');
    },
  });

  const activeSessionId = useRef(session.id);
  activeSessionId.current = session.id;
  const mounted = useRef(true);
  useEffect(() => () => {
    mounted.current = false;
  }, []);

  const attachmentImportTail = useRef<Promise<void>>(Promise.resolve());
  const pendingAttachmentImports = useRef(0);

  async function addLocalFiles(files: LocalAttachmentFile[]) {
    if (!files.length) {return;}
    const targetSessionId = session.id;
    pendingAttachmentImports.current += 1;
    setImportingAttachments(true);
    setLocalError(null);
    const operation = attachmentImportTail.current.catch(() => {}).then(async () => {
      const client = daemon.client;
      if (!client || daemon.phase !== 'connected') {
        throw new Error('Goddard daemon is disconnected');
      }

      for (const file of files) {
        const imported = await importLocalAttachment(client, file);
        if (mounted.current && activeSessionId.current === targetSessionId) {
          draftSync.markEdited();
          setAttachments((current) => [...current, imported]);
        }
      }
    });

    attachmentImportTail.current = operation;
    try {
      await operation;
      await Haptics.selectionAsync();
    } finally {
      pendingAttachmentImports.current -= 1;
      if (mounted.current && pendingAttachmentImports.current === 0) {
        setImportingAttachments(false);
      }
    }
  }

  async function chooseAttachment(source: ComposerAttachmentSource) {
    try {
      const files = await pickAttachments(source);
      if (files) {await addLocalFiles(files);}
    } catch (cause) {
      setLocalError(cause instanceof Error ? cause.message : String(cause));
      await Haptics.notificationAsync('error');
    }
  }

  const requestSignature = permission?.requestId ?? userInput?.requestId;
  const lastRequest = useRef<string | undefined>(undefined);
  useEffect(() => {
    if (requestSignature && requestSignature !== lastRequest.current) {
      void Haptics.notificationAsync('warning');
    }

    lastRequest.current = requestSignature;
  }, [requestSignature]);

  async function submit() {
    const value = draft.trim();
    // A supervision message carries the viewed employee's session
    // reference — its `[session … (task_id: …)]` token splices into the
    // provider prompt through the ordinary attachment path; an armed
    // deliverable rides along as its `@path` file attachment.
    const submittedAttachments = [
      ...attachments,
      ...(boss ? [sessionReferenceAttachment(boss.employee.sessionId, boss.employee.identity.name)] : []),
      ...(deliverable ? [deliverableAttachment(deliverable)] : []),
    ];

    const submittedAnnotations = draftSync.currentAnnotations();
    if (
      (!value && submittedAttachments.length === 0 && submittedAnnotations.length === 0)
      || submitting
      || pendingAttachmentImports.current > 0
    ) {return;}

    setSubmitting(true);
    setLocalError(null);
    try {
      const commands = value.startsWith('/') ? await contextPicker.getCommands() : [];
      const bareRename = parseRenameSubmission(value) === null;
      if (bareRename) {
        await runtime.grantAgentRename(session.id);
      } else if (await localCommands.execute(value, commands)) {return;}

      const expanded = composerProviderPrompt(session.provider, value, commands, submittedAttachments);
      let displayPrompt = value;
      let providerPrompt = bareRename
        ? [
            'You have permission to rename this task. First read this task\'s transcript with `goddard-agent read \'{}\'` to see its current title. Keep it unless the task has substantially changed or pivoted. If renaming, preserve its unique subject and describe the task\'s purpose, not recent steps or progress; use `goddard-agent rename`.',
            submittedAttachments.map(attachmentPromptToken).join(' '),
          ].filter(Boolean).join(' ')
        : expanded;

      if (submittedAnnotations.length) {
        displayPrompt = annotationBubbleContent(submittedAnnotations, value);
        const base = bareRename
          ? [
              'You have permission to rename this task. First read this task\'s transcript with `goddard-agent read \'{}\'` to see its current title. Keep it unless the task has substantially changed or pivoted. If renaming, preserve its unique subject and describe the task\'s purpose, not recent steps or progress; use `goddard-agent rename`.',
              submittedAttachments.map(attachmentPromptToken).join(' '),
            ].filter(Boolean).join(' ')
          : expanded ?? [
              value,
              submittedAttachments.map(attachmentPromptToken).join(' '),
            ].filter(Boolean).join(' ');

        providerPrompt = (annotationPromptPrefix(submittedAnnotations) + base).trimEnd();
      }

      onSubmitted?.();
      if (boss) {
        // The message goes to the boss chat — created on first use — and
        // the chat comes on screen so the command's destination is visible.
        const client = daemon.client;
        if (!client) {throw new Error('Goddard daemon is disconnected');}
        const { session: bossSession } = await openBossSession(client, {
          provider: session.provider,
          model: session.model ?? null,
          mode: session.runtime_mode,
        });

        await runtime.sendPrompt(bossSession, displayPrompt, submittedAttachments, providerPrompt);
        pushRoute({ stack: 'root', name: 'session/:id', params: { id: bossSession.id } });
      } else if (canSteer) {await runtime.steerPrompt(session, displayPrompt, submittedAttachments, providerPrompt);}
      else {await runtime.sendPrompt(session, displayPrompt, submittedAttachments, providerPrompt);}

      draftSync.removeSubmittedDraft();
      setDraft('');
      setAttachments([]);
      if (deliverable) {clearDeliverableArm();}
      await Haptics.selectionAsync();
    } catch (cause) {
      setLocalError(cause instanceof Error ? cause.message : String(cause));
      await Haptics.notificationAsync('error');
    } finally {
      setSubmitting(false);
    }
  }

  async function stop() {
    setLocalError(null);
    try {
      await runtime.cancel(session.id);
      await Haptics.notificationAsync('warning');
    } catch (cause) {
      setLocalError(cause instanceof Error ? cause.message : String(cause));
    }
  }

  function applyOptions(changes: Parameters<typeof runtime.updateSessionOptions>[1]) {
    runtime.updateSessionOptions(session.id, changes).catch((cause) => {
      setLocalError(cause instanceof Error ? cause.message : String(cause));
    });
  }

  const disconnected = daemon.phase !== 'connected';
  const placeholder = boss
    ? 'Use the boss to control this employee…'
    : deliverable
      ? 'Ask the boss about this deliverable…'
    : disconnected
      ? daemon.phase === 'reconnecting'
        ? 'Reconnecting…'
        : daemon.phase === 'connecting' || daemon.phase === 'booting'
          ? 'Connecting…'
          : 'Reconnect to message this agent'
      : canSteer
        ? 'Message the working agent…'
        : busy
          ? 'Queue a follow-up…'
          : 'Message agent';

  return (
    <View style={[{ paddingHorizontal: 12, paddingTop: 4 }, shellPadding]}>
      {permission && !userInput && (
        <PermissionPanel
          permission={permission}
          onRespond={onPermissionRespond ?? ((optionId) => runtime.respond(session.id, permission.requestId, optionId))}
        />
      )}
      {userInput && (
        <UserInputPanel
          input={userInput}
          supportsActions={Boolean(liveRuntime?.supportsUserInputActions)}
          onSubmit={(answers) => runtime.respondUserInput(session.id, userInput.requestId, answers)}
          onClarify={(content) => runtime.clarifyUserInput(session.id, userInput.requestId, content)}
          onDismiss={() => runtime.cancelUserInput(session.id, userInput.requestId)}
        />
      )}
      {visibleError && (
        <View
          accessibilityLiveRegion="polite"
          style={{
            alignItems: 'center',
            borderRadius: Radius.small,
            flexDirection: 'row',
            gap: 9,
            marginBottom: 7,
            paddingHorizontal: 10,
            paddingVertical: 7,
            backgroundColor: theme.dangerSoft,
          }}>
          <Text style={{ flex: '1 1', fontSize: 13, fontWeight: '600', lineHeight: '17px', color: theme.danger }}>
            {visibleError}
          </Text>
          <Pressable
            accessibilityLabel="Dismiss error"
            accessibilityRole="button"
            onPress={() => {
              setLocalError(null);
              runtime.dismissError(session.id);
            }}>
            <AppSymbol
              name={{ ios: 'xmark', android: 'close', web: 'close' }}
              size={12}
              tintColor={theme.danger}
            />
          </Pressable>
        </View>
      )}
      {queued.map((message) => (
        <View
          style={{
            alignItems: 'center',
            borderRadius: Radius.small,
            borderWidth: hairline,
            flexDirection: 'row',
            gap: 8,
            marginBottom: 6,
            minHeight: 34,
            paddingHorizontal: 10,
            backgroundColor: theme.overlay,
            borderColor: theme.border,
          }}>
          <AppSymbol
            name={
              isAgentQueuedMessage(message)
                ? { ios: 'cpu', android: 'smart_toy', web: 'smart_toy' }
                : { ios: 'clock', android: 'schedule', web: 'schedule' }
            }
            size={12}
            tintColor={theme.textTertiary}
          />
          <Text numberOfLines={1} style={{ flex: '1 1', fontSize: 13.5, color: theme.textSecondary }}>
            {atomVisibleText(message.display_content ?? '').trim()
              || message.attachments?.map((attachment) => attachment.name).join(', ')
              || message.content}
          </Text>
          <Pressable
            accessibilityLabel="Remove queued message"
            accessibilityRole="button"
            onPress={() => void runtime.removeQueuedMessage(session.id, message.id).catch(() => {})}>
            <AppSymbol
              name={{ ios: 'xmark', android: 'close', web: 'close' }}
              size={11}
              tintColor={theme.textTertiary}
            />
          </Pressable>
        </View>
      ))}

      {boss && (
        <View
          accessibilityLiveRegion="polite"
          style={{
            alignItems: 'center',
            borderRadius: Radius.small,
            flexDirection: 'row',
            gap: 7,
            marginBottom: 7,
            paddingHorizontal: 10,
            paddingVertical: 6,
            backgroundColor: theme.overlay,
          }}>
          <BossAvatar identity={boss.identity} size={16} />
          <Text style={{ flex: '1 1', fontSize: 13, color: theme.textSecondary }}>
            Messages go to {boss.identity.name}
          </Text>
        </View>
      )}

      <ComposerCard
        {...contextPicker.inputProps}
        beforeInput={attachments.length || importingAttachments || deliverable ? (
          <ScrollView
            horizontal
            style={{ marginBottom: 6 }}>
            <View style={{ flexDirection: 'row', gap: 8, paddingHorizontal: 4, paddingTop: 4, paddingBottom: 6 }}>
              {deliverable ? (
                <Pressable
                  accessibilityHint="Opens the deliverable preview"
                  accessibilityLabel={`Deliverable ${deliverable.name}`}
                  accessibilityRole="button"
                  onPress={showArmedDeliverablePreview}
                  style={{
                    alignItems: 'center',
                    alignSelf: 'center',
                    borderColor: theme.border,
                    borderRadius: 8,
                    borderWidth: 1,
                    flexDirection: 'row',
                    gap: 5,
                    height: 24,
                    maxWidth: 240,
                    paddingLeft: 6,
                    paddingRight: 4,
                    backgroundColor: theme.inset,
                  }}>
                  <AppSymbol
                    name={deliverable.directory
                      ? { ios: 'folder', android: 'folder', web: 'folder' }
                      : { ios: 'doc', android: 'description', web: 'description' }}
                    size={11}
                    tintColor={theme.textTertiary}
                  />
                  <Text
                    numberOfLines={1}
                    style={{ flexShrink: 1, fontSize: 13.5, color: theme.textSecondary }}>
                    {deliverable.name}
                  </Text>
                  <Pressable
                    accessibilityLabel={`Remove ${deliverable.name}`}
                    accessibilityRole="button"
                    disabled={submitting}
                    onPress={clearDeliverableArm}
                    style={{
                      alignItems: 'center',
                      height: 28,
                      justifyContent: 'center',
                      opacity: submitting ? 0.4 : 1,
                      width: 28,
                    }}>
                    <AppSymbol name={{ ios: 'xmark', android: 'close', web: 'close' }} size={9} tintColor={theme.textSecondary} />
                  </Pressable>
                </Pressable>
              ) : null}
              {attachments.map((attachment, index) => (
                <AttachmentTile
                  attachment={attachment}
                  compact
                  onRemove={() => {
                    draftSync.markEdited();
                    setAttachments((current) => current.filter((_, item) => item !== index));
                  }}
                  removeDisabled={submitting}
                />
              ))}
              {importingAttachments && (
                <View style={{
                  alignItems: 'center',
                  borderRadius: 9,
                  borderWidth: 1,
                  justifyContent: 'center',
                  gap: 7,
                  height: 80,
                  width: 80,
                  backgroundColor: theme.inset,
                  borderColor: theme.border,
                }}>
                  <ActivityIndicator busy />
                  <Text style={{ fontSize: 12.5, color: theme.textSecondary }}>Attaching…</Text>
                </View>
              )}
            </View>
          </ScrollView>
        ) : undefined}
        editable={!disconnected && !submitting}
        left={(
          <>
            <ComposerAddMenu
              disabled={disconnected || submitting || importingAttachments}
              onChoose={(source) => void chooseAttachment(source)}
              onChooseContext={contextPicker.open}
            />
            <ComposerAccessMenu
              mode={session.runtime_mode}
              onApply={(mode) => applyOptions({ runtimeMode: mode })}
            />
          </>
        )}
        placeholder={placeholder}
        right={(
          <>
            {activeModel && modelHasConfigurableTraits(activeModel) && (
              <ComposerIconButton
                icon={{ ios: 'speedometer', android: 'speed', web: 'speed' }}
                label="Model options"
                onPress={() => setTraitsSheetOpen(true)}
              />
            )}
            {busy && (
              <Pressable
                accessibilityLabel="Stop agent"
                accessibilityRole="button"
                onPress={() => void stop()}
                style={{
                  alignItems: 'center',
                  borderRadius: Radius.pill,
                  height: 36,
                  justifyContent: 'center',
                  marginLeft: 4,
                  width: 36,
                  backgroundColor: theme.inverse,
                }}>
                <AppSymbol
                  name={{ ios: 'stop.fill', android: 'stop', web: 'stop' }}
                  size={14}
                  tintColor={theme.onInverse}
                />
              </Pressable>
            )}
            <SendButton
              busy={submitting}
              disabled={
                // A bare session reference is a complete supervision
                // command — the boss reads "look at this employee" from it.
                (!draft.trim()
                  && attachments.length === 0
                  && !boss
                  && draftSync.currentAnnotations().length === 0)
                || submitting
                || importingAttachments
                || disconnected
              }
              label={canSteer ? 'Send to working agent' : busy ? 'Queue message' : 'Send message'}
              onPress={() => void submit()}
              queueing={busy && !canSteer}
              steering={canSteer}
            />
          </>
        )}
        value={draft}
        onPasteError={(message) => {
          setLocalError(message);
          void Haptics.notificationAsync('error');
        }}
        onPasteFiles={(files) => {
          void addLocalFiles(files).catch(async (cause) => {
            setLocalError(cause instanceof Error ? cause.message : String(cause));
            await Haptics.notificationAsync('error');
          });
        }}
      />

      {activeModel && (
        <ModelTraitsSheet
          model={activeModel}
          onApply={applyOptions}
          onDismiss={() => setTraitsSheetOpen(false)}
          selection={{
            reasoningEffort: session.reasoning_effort ?? null,
            serviceTier: session.service_tier ?? null,
            contextWindow: session.context_window ?? null,
          }}
          visible={traitsSheetOpen}
        />
      )}
      {localCommands.sheets}
      <ComposerContextPicker {...contextPicker.picker} />
    </View>
  );
}

export function PermissionPanel({
  permission,
  onRespond,
}: {
  permission: PendingPermission;
  onRespond: (optionId: string) => Promise<void>;
}) {
  const theme = useTheme();
  const [responding, setResponding] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  return (
    <RequestPanel borderColor={theme.warning}>
      <View style={{ alignItems: 'center', flexDirection: 'row', gap: 8 }}>
        <AppSymbol
          name={{ ios: 'hand.raised.fill', android: 'front_hand', web: 'pan_tool' }}
          size={16}
          tintColor={theme.warning}
        />
        <Text style={{ flex: '1 1', fontSize: 15, fontWeight: '700', color: theme.text }}>{permission.title}</Text>
      </View>
      {permission.detail ? (
        <ScrollView
          style={{
            borderRadius: Radius.small,
            borderWidth: hairline,
            marginTop: 9,
            maxHeight: 110,
            padding: 9,
            backgroundColor: theme.inset,
            borderColor: theme.border,
          }}>
          <Text style={{ fontFamily: MonoFont, fontSize: 12.5, lineHeight: '17px', color: theme.textSecondary }}>
            {permission.detail}
          </Text>
        </ScrollView>
      ) : null}
      {error && <Text style={{ fontSize: 13, lineHeight: '17px', marginTop: 8, color: theme.danger }}>{error}</Text>}
      <View style={{ flexDirection: 'row', flexWrap: 'wrap', gap: 8, justifyContent: 'flex-end', marginTop: 11 }}>
        {permission.options.map((option) => (
          <Pressable
            accessibilityRole="button"
            disabled={Boolean(responding)}
            onPress={() => {
              setResponding(option.id);
              setError(null);
              void Haptics.selectionAsync();
              void onRespond(option.id).catch((cause) => {
                setError(cause instanceof Error ? cause.message : String(cause));
                setResponding(null);
              });
            }}
            style={{
              alignItems: 'center',
              borderRadius: Radius.pill,
              flexDirection: 'row',
              gap: 6,
              minHeight: 38,
              paddingHorizontal: 14,
              backgroundColor: option.allow ? theme.inverse : theme.surfaceMuted,
              opacity: responding && responding !== option.id ? 0.55 : 1,
            }}>
            {responding === option.id && <ActivityIndicator busy />}
            <Text style={{
              fontSize: 14,
              fontWeight: '700',
              color: option.allow ? theme.onInverse : theme.text,
            }}>{option.label}</Text>
          </Pressable>
        ))}
      </View>
    </RequestPanel>
  );
}

function UserInputPanel({
  input,
  supportsActions,
  onSubmit,
  onClarify,
  onDismiss,
}: {
  input: PendingUserInput;
  supportsActions: boolean;
  onSubmit: (answers: UserInputAnswer[]) => Promise<void>;
  onClarify: (content: string) => Promise<void>;
  onDismiss: () => Promise<void>;
}) {
  const theme = useTheme();
  const [index, setIndex] = useState(0);
  const [selections, setSelections] = useState<Record<string, string[]>>({});
  const [customAnswers, setCustomAnswers] = useState<Record<string, string>>({});
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setIndex(0);
    setSelections({});
    setCustomAnswers({});
    setSubmitting(false);
    setError(null);
  }, [input.requestId]);

  const question = input.questions[index];
  if (!question) {return null;}
  const selected = selections[question.id] ?? [];
  const custom = customAnswers[question.id] ?? '';
  const canContinue = Boolean(custom.trim() || selected.length);
  const last = index === input.questions.length - 1;

  function toggle(label: string) {
    void Haptics.selectionAsync();
    setCustomAnswers((values) => ({ ...values, [question.id]: '' }));
    setSelections((values) => {
      const previous = values[question.id] ?? [];
      return {
        ...values,
        [question.id]: question.multiSelect
          ? previous.includes(label)
            ? previous.filter((value) => value !== label)
            : [...previous, label]
          : [label],
      };
    });
  }

  async function advance() {
    if (!canContinue || submitting) {return;}
    if (!last) {
      setIndex((value) => value + 1);
      return;
    }

    setSubmitting(true);
    setError(null);
    try {
      await onSubmit(input.questions.map((item) => {
        const customValue = customAnswers[item.id]?.trim();
        return {
          questionId: item.id,
          answers: customValue ? [customValue] : selections[item.id] ?? [],
        };
      }));
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
      setSubmitting(false);
    }
  }

  return (
    <RequestPanel borderColor={theme.accent}>
      <View style={{ alignItems: 'center', flexDirection: 'row', justifyContent: 'space-between' }}>
        <Text style={{ fontSize: 12, fontWeight: '700', letterSpacing: 0.55, textTransform: 'uppercase', color: theme.textTertiary }}>
          {question.header}
        </Text>
        {input.questions.length > 1 && (
          <Text style={{ fontSize: 12, fontWeight: '600', color: theme.textTertiary }}>
            {index + 1} of {input.questions.length}
          </Text>
        )}
      </View>
      <Text style={{ fontSize: 15, fontWeight: '600', lineHeight: '20px', marginTop: 7, color: theme.text }}>{question.question}</Text>
      <ScrollView style={{ marginTop: 10, maxHeight: 260 }}>
        {question.options.map((option) => {
          const checked = selected.includes(option.label);
          return (
            <Pressable
              accessibilityRole={question.multiSelect ? 'checkbox' : 'radio'}
              accessibilityState={{ checked }}
              onPress={() => toggle(option.label)}
              style={{
                alignItems: 'center',
                borderRadius: Radius.medium,
                borderWidth: 1,
                flexDirection: 'row',
                gap: 8,
                marginBottom: 6,
                minHeight: 44,
                paddingHorizontal: 11,
                paddingVertical: 8,
                backgroundColor: checked ? theme.overlayStrong : theme.surfaceMuted,
                borderColor: checked ? NativeTint : 'transparent',
              }}>
              <View style={{ flex: '1 1' }}>
                <Text style={{ fontSize: 14, fontWeight: '600', color: theme.text }}>{option.label}</Text>
                {option.description && option.description !== option.label && (
                  <Text style={{ fontSize: 12.5, lineHeight: '16px', marginTop: 2, color: theme.textSecondary }}>
                    {option.description}
                  </Text>
                )}
              </View>
              {checked && (
                <AppSymbol
                  name={{ ios: 'checkmark', android: 'check', web: 'check' }}
                  size={14}
                  tintColor={NativeTint as string}
                />
              )}
            </Pressable>
          );
        })}
        <TextArea
          placeholder="Write another answer…"
          rows={2}
          style={{
            borderRadius: Radius.medium,
            borderWidth: 1,
            fontSize: 14,
            lineHeight: '18px',
            minHeight: 42,
            paddingHorizontal: 11,
            paddingVertical: 9,
            backgroundColor: theme.surfaceMuted,
            borderColor: custom.trim() ? NativeTint : 'transparent',
            color: theme.text,
          }}
          value={custom}
          onChange={(value) => {
            setCustomAnswers((values) => ({ ...values, [question.id]: value }));
            if (value.trim()) {setSelections((values) => ({ ...values, [question.id]: [] }));}
          }}
        />
      </ScrollView>
      {error && <Text style={{ fontSize: 13, lineHeight: '17px', marginTop: 8, color: theme.danger }}>{error}</Text>}
      {supportsActions && (
        <View style={{ alignItems: 'center', flexDirection: 'row', justifyContent: 'space-between', marginTop: 10 }}>
          <Pressable
            accessibilityHint="Dismisses these questions without answering"
            accessibilityRole="button"
            disabled={submitting}
            onPress={() => {
              setSubmitting(true);
              setError(null);
              void Haptics.selectionAsync();
              void onDismiss().catch((cause) => {
                setError(cause instanceof Error ? cause.message : String(cause));
                setSubmitting(false);
              });
            }}
            style={{ justifyContent: 'center', minHeight: 36, paddingHorizontal: 6 }}>
            <Text style={{ fontSize: 14, fontWeight: '600', color: theme.textTertiary }}>Dismiss</Text>
          </Pressable>
          <Pressable
            accessibilityHint="Sends your note back so the agent can re-ask"
            accessibilityRole="button"
            disabled={!custom.trim() || submitting}
            onPress={() => {
              setSubmitting(true);
              setError(null);
              void Haptics.selectionAsync();
              void onClarify(custom.trim()).catch((cause) => {
                setError(cause instanceof Error ? cause.message : String(cause));
                setSubmitting(false);
              });
            }}
            style={{
              alignItems: 'center',
              borderRadius: 8,
              borderWidth: 1,
              justifyContent: 'center',
              minHeight: 30,
              paddingHorizontal: 10,
              borderColor: custom.trim() ? theme.borderStrong : theme.border,
              opacity: !custom.trim() || submitting ? 0.55 : 1,
            }}>
            <Text style={{
              fontSize: 12.5,
              fontWeight: '600',
              color: custom.trim() ? theme.text : theme.textGhost,
            }}>Clarify</Text>
          </Pressable>
        </View>
      )}
      <View style={{ alignItems: 'center', flexDirection: 'row', justifyContent: 'space-between', marginTop: 10 }}>
        {index > 0 ? (
          <Pressable
            accessibilityRole="button"
            onPress={() => setIndex((value) => value - 1)}
            style={{ justifyContent: 'center', minHeight: 36, paddingHorizontal: 6 }}>
            <Text style={{ fontSize: 14, fontWeight: '600', color: theme.textSecondary }}>Back</Text>
          </Pressable>
        ) : <View />}
        <Pressable
          accessibilityRole="button"
          disabled={!canContinue || submitting}
          onPress={() => void advance()}
          style={{
            alignItems: 'center',
            borderRadius: Radius.pill,
            flexDirection: 'row',
            gap: 6,
            justifyContent: 'center',
            minHeight: 36,
            minWidth: 78,
            paddingHorizontal: 14,
            backgroundColor: theme.inverse,
            opacity: !canContinue || submitting ? 0.55 : 1,
          }}>
          {submitting && <ActivityIndicator busy />}
          <Text style={{ fontSize: 14, fontWeight: '700', color: theme.onInverse }}>
            {last ? 'Submit' : 'Next'}
          </Text>
        </Pressable>
      </View>
    </RequestPanel>
  );
}

function RequestPanel({ borderColor, children }: { borderColor: string; children: ReactNode }) {
  const theme = useTheme();
  return (
    <View style={{
      borderRadius: Radius.large,
      borderWidth: 1,
      marginBottom: 8,
      padding: 12,
      backgroundColor: theme.surface,
      borderColor,
    }}>
      {children}
    </View>
  );
}
