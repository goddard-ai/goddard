import type { AgentSession, BossDeliverable, BossState } from '@waku/client';

import { share } from '@octane-xplat/share';
import {
  archiveDeliverable,
  deliverableUnread,
  dismissDeliverable,
  markDeliverableViewed,
  openBossSession,
  pinDeliverable,
  sweepDeliverable,
} from '@waku/client/boss';

import { memo, useCallback, useMemo, useState } from 'octane';
import {
  ActivityIndicator,
  ContextMenu,
  Image,
  Pressable,
  pushRoute,
  Text,
  useRoute,
  useStore,
  View,
} from '@octane-xplat/ui';

import { AppSymbol } from '@/components/app-symbol';
import { MdBlockSettled, useMarkdownStyles } from '@/components/md-block-row';
import { Sheet } from '@/components/sheet';
import { showAlert, confirmDestructive } from '@/lib/alert';
import { setStringAsync as copyToClipboard } from '@/lib/clipboard';
import {
  daemonKeys,
  readWorkspaceBinaryFile,
  readWorkspaceTextFile,
} from '@/lib/daemon-api';

import { useDaemon, useDaemonQuery } from '@/lib/daemon-hooks';
import {
  DELIVERABLE_RECENT_VISIBLE,
  deliverableFileName,
  deliverablePreviewKind,
  deliverableProducer,
  deliverableReadTarget,
  recentDeliverables,
} from '@/lib/deliverables';

import {
  armDeliverable,
  deliverableArm,
  type DeliverableArm,
} from '@/lib/deliverable-arm';

import { useProviderCatalog } from '@/lib/daemon-queries';
import { Haptics } from '@/lib/haptics';
import { imageMimeForPath } from '@/lib/task-surfaces';
import { Radius } from '@/lib/theme-colors';
import { MonoFont } from '@/lib/theme-platform';
import { relativeSessionTime } from '@/lib/session-presentation';
import { useTheme } from '@/lib/use-theme';
import { parseBlocks } from '@/md/parse';

/** Subscribed read of the armed deliverable — the boss chat's composer and
 * preview sheet both read it. */
export function useDeliverableArm(): DeliverableArm | null {
  return useStore(deliverableArm);
}

/** A deliverable row's tap: stamps the record viewed on the daemon, opens
 * the publishing boss chat (creating it on first use), arms the file as
 * composer context, and opens the preview sheet on top. Mirrors the
 * desktop's `open_deliverable_task`. */
export function useOpenDeliverable(
  state: BossState | null,
  onNavigate?: () => void,
): (deliverable: BossDeliverable) => void {
  const daemon = useDaemon();
  const catalog = useProviderCatalog();
  const route = useRoute('root');
  const currentSessionId = route?.name === 'session/:id'
    ? (route.params as { id?: string }).id
    : undefined;

  return useCallback(
    (deliverable: BossDeliverable) => {
      const client = daemon.client;
      if (!client) {return;}

      void markDeliverableViewed(client, deliverable.id).catch(() => {});

      const open = async (): Promise<string> => {
        if (state?.sessionId) {return state.sessionId;}
        // Provider/mode only matter when this call creates the chat — the
        // same preference order the roster's boss-chat row uses.
        const installed = catalog.providers
          .filter((item) => item.installed)
          .map((item) => item.id);

        const provider = installed.includes('codex') ? 'codex' : installed[0] ?? 'codex';
        const { session } = await openBossSession(client, {
          provider,
          model: null,
          mode: 'autoAcceptEdits',
        });

        return session.id;
      };

      void open()
        .then((sessionId) => {
          onNavigate?.();
          if (currentSessionId !== sessionId) {
            pushRoute({ stack: 'root', name: 'session/:id', params: { id: sessionId } });
          }

          armDeliverable({ deliverable, sessionId, previewOpen: true });
        })
        .catch((cause) => {
          void showAlert(
            'Couldn’t open the boss chat',
            cause instanceof Error ? cause.message : String(cause),
          );
        });
    },
    [catalog.providers, currentSessionId, daemon.client, onNavigate, state?.sessionId],
  );
}

/** Row affordances shared by the sidebar group and the library — every
 * mutation is a boss operation on the owning daemon, returning through the
 * usual `taskStateChanged` refetch. */
export function useDeliverableActions(deliverable: BossDeliverable): {
  menuItems: Array<{
    key: string;
    label: string;
    icon?: string;
    onSelect: () => void;
  }>;
} {
  const daemon = useDaemon();

  const run = useCallback(
    (label: string, op: (client: NonNullable<typeof daemon.client>) => Promise<void>) => {
      const client = daemon.client;
      if (!client) {return;}
      void Haptics.selectionAsync();
      void op(client).catch((cause) => {
        void showAlert(label, cause instanceof Error ? cause.message : String(cause));
      });
    },
    [daemon.client],
  );

  return useMemo(() => {
    const pinned = deliverable.pinnedAt != null;
    const dormant = deliverable.dormantAt != null && !pinned;
    const archived = deliverable.archivedAt != null;

    const shareDeliverable = () => {
      const client = daemon.client;
      if (!client) {return;}
      void (async () => {
        // Text files share their content; anything else shares its name
        // and daemon-host path so the sheet's intent still lands.
        let text = `${deliverable.name}\n${deliverable.path}`;
        if (!deliverable.directory && imageMimeForPath(deliverable.path) == null) {
          const target = deliverableReadTarget(deliverable);
          if (target.relativePath != null) {
            try {
              text = await readWorkspaceTextFile(client, target.root, target.relativePath);
            } catch {
              // Binary or unreadable — fall back to the name/path text.
            }
          }
        }

        const result = await share.text(text, deliverable.name);
        if (result === 'unavailable' || result === 'copied') {
          await copyToClipboard(text);
          if (result === 'unavailable') {
            void showAlert('Sharing unavailable', 'Copied the deliverable text instead.');
          }
        }
      })().catch((cause) => {
        void showAlert('Couldn’t share', cause instanceof Error ? cause.message : String(cause));
      });
    };

    const menuItems: Array<{ key: string; label: string; icon?: string; onSelect: () => void }> = [
      {
        key: 'pin',
        label: pinned ? 'Unpin' : 'Pin',
        icon: pinned ? 'pin.slash' : 'pin',
        onSelect: () => run('Couldn’t update pin', (client) => pinDeliverable(client, deliverable.id, !pinned)),
      },
      {
        key: 'sweep',
        label: dormant ? 'Restore' : 'Sweep',
        icon: dormant ? 'arrow.uturn.left' : 'tray.and.arrow.down',
        onSelect: () => run('Couldn’t update deliverable', (client) => sweepDeliverable(client, deliverable.id, !dormant)),
      },
      {
        key: 'archive',
        label: archived ? 'Unarchive' : 'Archive',
        icon: archived ? 'arrow.up.bin' : 'archivebox',
        onSelect: () => run('Couldn’t update archive', (client) => archiveDeliverable(client, deliverable.id, !archived)),
      },
      {
        key: 'share',
        label: 'Share',
        icon: 'square.and.arrow.up',
        onSelect: shareDeliverable,
      },
      {
        key: 'dismiss',
        label: 'Dismiss',
        icon: 'trash',
        onSelect: () => {
          void confirmDestructive(
            `Dismiss “${deliverable.name}”?`,
            'This removes the record and its stored copy from the daemon.',
            'Dismiss',
          ).then((confirmed) => {
            if (!confirmed) {return;}
            run('Couldn’t dismiss deliverable', (client) => dismissDeliverable(client, deliverable.id));
          });
        },
      },
    ];

    return { menuItems };
  }, [daemon.client, deliverable, run]);
}

const FILE_ROW_DETAIL_ICON = {
  file: { ios: 'doc', android: 'draft', web: 'draft' },
  directory: { ios: 'folder', android: 'folder', web: 'folder' },
} as const;

/** One deliverable row — the sidebar group's card and the library's list
 * row share this: name on the title line, icon + file name (+ host) on the
 * detail line, age trailing, unread dot, pin glyph. Long-press opens the
 * context menu; tap opens the boss chat with the file armed. */
export const DeliverableRow = memo(function DeliverableRow({
  deliverable,
  host,
  now,
  statusTag,
  onPress,
  menuItems,
}: {
  deliverable: BossDeliverable;
  /** Daemon profile name — paths live on the daemon host, not this device. */
  host?: string | null;
  now: number;
  /** Library rows name their lifecycle state since no fold explains it. */
  statusTag?: string | null;
  onPress: () => void;
  menuItems: Array<{ key: string; label: string; icon?: string; onSelect: () => void }>;
}) {
  const theme = useTheme();
  const unread = deliverableUnread(deliverable);
  const pinned = deliverable.pinnedAt != null;
  const fileName = host
    ? `${deliverableFileName(deliverable)} · ${host}`
    : deliverableFileName(deliverable);

  const detailIcon = deliverable.directory
    ? FILE_ROW_DETAIL_ICON.directory
    : FILE_ROW_DETAIL_ICON.file;

  return (
    <ContextMenu items={menuItems}>
      <Pressable
        accessibilityHint="Long press for actions"
        accessibilityLabel={`${deliverable.name}, ${fileName}${unread ? ', unread' : ''}`}
        accessibilityRole="button"
        className="vx-pressable"
        onPress={onPress}
        style={{
          borderRadius: Radius.medium,
          gap: 3,
          marginHorizontal: 6,
          minHeight: 46,
          paddingHorizontal: 8,
          paddingVertical: 7,
        }}
      >
        <View style={{ alignItems: 'center', flexDirection: 'row', gap: 6 }}>
          <Text
            numberOfLines={1}
            style={{ color: theme.text, flex: '1 1', fontSize: 13.5, fontWeight: '500', minWidth: 0 }}
          >
            {deliverable.name}
          </Text>
          {unread ? (
            <View style={{ backgroundColor: theme.info, borderRadius: 3.5, height: 7, width: 7 }} />
          ) : null}
        </View>
        <View style={{ alignItems: 'center', flexDirection: 'row', gap: 5, minHeight: 15 }}>
          <AppSymbol name={detailIcon} size={12} tintColor={theme.textTertiary} />
          <Text
            numberOfLines={1}
            style={{ color: theme.textTertiary, flexShrink: 1, fontSize: 12.5, minWidth: 0 }}
          >
            {fileName}
          </Text>
          {statusTag ? (
            <Text style={{ color: theme.textGhost, flexShrink: 0, fontSize: 12.5 }}>
              {statusTag}
            </Text>
          ) : null}
          <View style={{ flex: '1 1' }} />
          {pinned ? (
            <AppSymbol
              name={{ ios: 'pin.fill', android: 'push_pin', web: 'push_pin' }}
              size={11}
              tintColor={theme.textTertiary}
            />
          ) : null}
          <Text style={{ color: theme.textTertiary, fontSize: 12 }}>
            {relativeSessionTime(deliverable.updatedAt, now)}
          </Text>
        </View>
      </Pressable>
    </ContextMenu>
  );
});

/** The drawer's Deliverables group — the desktop's Recent deliverables
 * sidebar group: live rows (pinned lead, then newest) capped behind "Show
 * more", swept rows behind "Show dormant", and a "See all" entry to the
 * library. Hidden entirely while the boss has published nothing. */
export function DeliverablesSection({
  state,
  host,
  now,
  onClose,
}: {
  state: BossState;
  host?: string | null;
  now: number;
  onClose: () => void;
}) {
  const theme = useTheme();
  const [showMore, setShowMore] = useState(false);
  const [showDormant, setShowDormant] = useState(false);
  const openDeliverable = useOpenDeliverable(state, onClose);

  const { live, dormant } = useMemo(
    () => recentDeliverables(state.deliverables, Math.floor(now / 1_000)),
    [state.deliverables, now],
  );

  const visible = showMore ? live : live.slice(0, DELIVERABLE_RECENT_VISIBLE);
  const hiddenCount = live.length - visible.length;

  const openLibrary = useCallback(() => {
    onClose();
    pushRoute({ stack: 'root', name: 'deliverables', params: {} });
  }, [onClose]);

  if (state.deliverables.length === 0) {return null;}

  return (
    <View>
      <View
        style={{
          alignItems: 'center',
          flexDirection: 'row',
          marginTop: 10,
          marginBottom: 4,
          paddingHorizontal: 14,
        }}
      >
        <Text
          style={{
            color: theme.textTertiary,
            flex: '1 1',
            fontSize: 13,
            fontWeight: '600',
            textTransform: 'uppercase',
          }}
        >
          Deliverables
        </Text>
        <Pressable
          accessibilityHint="Opens the deliverables library"
          accessibilityLabel="See all deliverables"
          accessibilityRole="button"
          onPress={openLibrary}
          style={{ alignItems: 'center', flexDirection: 'row', gap: 2 }}
        >
          <Text style={{ color: theme.textTertiary, fontSize: 12.5 }}>See all</Text>
          <AppSymbol
            name={{ ios: 'chevron.right', android: 'chevron_right', web: 'chevron_right' }}
            size={11}
            tintColor={theme.textTertiary}
          />
        </Pressable>
      </View>

      {live.length === 0 && dormant.length === 0 ? (
        <Pressable
          accessibilityHint="Opens the deliverables library"
          accessibilityRole="button"
          className="vx-pressable"
          onPress={openLibrary}
          style={{ marginHorizontal: 6, paddingHorizontal: 8, paddingVertical: 8 }}
        >
          <Text style={{ color: theme.textTertiary, fontSize: 13.5 }}>
            Nothing recent — browse the library
          </Text>
        </Pressable>
      ) : null}

      {visible.map((deliverable) => (
        <SectionDeliverableRow
          deliverable={deliverable}
          host={host}
          now={now}
          onOpen={openDeliverable}
        />
      ))}

      {hiddenCount > 0 && !showMore ? (
        <Pressable
          accessibilityRole="button"
          className="vx-pressable"
          onPress={() => setShowMore(true)}
          style={{ paddingHorizontal: 14, paddingVertical: 8 }}
        >
          <Text style={{ color: theme.textSecondary, fontSize: 13.5, fontWeight: '500' }}>
            Show more ({hiddenCount} older)
          </Text>
        </Pressable>
      ) : null}
      {showMore && hiddenCount <= 0 && live.length > DELIVERABLE_RECENT_VISIBLE ? (
        <Pressable
          accessibilityRole="button"
          className="vx-pressable"
          onPress={() => setShowMore(false)}
          style={{ paddingHorizontal: 14, paddingVertical: 8 }}
        >
          <Text style={{ color: theme.textSecondary, fontSize: 13.5, fontWeight: '500' }}>
            Show less
          </Text>
        </Pressable>
      ) : null}

      {dormant.length > 0 ? (
        <>
          <Pressable
            accessibilityRole="button"
            className="vx-pressable"
            onPress={() => setShowDormant((current) => !current)}
            style={{ paddingHorizontal: 14, paddingVertical: 8 }}
          >
            <Text style={{ color: theme.textSecondary, fontSize: 13.5, fontWeight: '500' }}>
              {showDormant
                ? 'Hide dormant'
                : `Show dormant (${dormant.length})`}
            </Text>
          </Pressable>
          {showDormant
            ? dormant.map((deliverable) => (
                <SectionDeliverableRow
                  deliverable={deliverable}
                  host={host}
                  now={now}
                  onOpen={openDeliverable}
                />
              ))
            : null}
        </>
      ) : null}
    </View>
  );
}

function SectionDeliverableRow({
  deliverable,
  host,
  now,
  onOpen,
}: {
  deliverable: BossDeliverable;
  host?: string | null;
  now: number;
  onOpen: (deliverable: BossDeliverable) => void;
}) {
  const { menuItems } = useDeliverableActions(deliverable);
  return (
    <DeliverableRow
      deliverable={deliverable}
      host={host}
      menuItems={menuItems}
      now={now}
      onPress={() => onOpen(deliverable)}
    />
  );
}

const MAX_PREVIEW_CHARACTERS = 250_000;

/** The armed deliverable's preview sheet — mounts over the boss chat it
 * armed, mirroring the desktop's deliverable page. Markdown renders, images
 * paint, other text reads plainly; directories and unreadable bytes show
 * the record's identity plus why there is no preview — never blank. */
export function DeliverableSheet({
  state,
  deliverable,
  host,
  sessions,
  visible,
  onDismiss,
}: {
  state: BossState | null;
  deliverable: BossDeliverable;
  host?: string | null;
  sessions: AgentSession[];
  visible: boolean;
  onDismiss: () => void;
}) {
  const theme = useTheme();
  const daemon = useDaemon();
  const kind = deliverablePreviewKind(deliverable);
  const target = deliverableReadTarget(deliverable);
  const profileId = daemon.activeProfile?.id ?? 'disconnected';

  const text = useDaemonQuery(
    [...daemonKeys.workspaceFile(profileId, target.root, target.relativePath ?? 'none'), 'deliverable'],
    () => readWorkspaceTextFile(daemon.client!, target.root, target.relativePath!),
    {
      enabled: Boolean(
        visible
          && daemon.client
          && daemon.phase === 'connected'
          && kind !== 'directory'
          && kind !== 'image'
          && target.relativePath,
      ),
    },
  );

  const image = useDaemonQuery(
    [...daemonKeys.workspaceFile(profileId, target.root, target.relativePath ?? 'none'), 'binary'],
    () => readWorkspaceBinaryFile(daemon.client!, target.root, target.relativePath!),
    {
      enabled: Boolean(
        visible
          && daemon.client
          && daemon.phase === 'connected'
          && kind === 'image'
          && target.relativePath,
      ),
    },
  );

  const producer = useMemo(
    () => (state ? deliverableProducer(state, deliverable, sessions) : null),
    [state, deliverable, sessions],
  );

  const origin = [
    producer ? `${producer.identity.name} · ${producer.jobTitle}` : null,
    relativeSessionTime(deliverable.updatedAt, Date.now()),
  ]
    .filter(Boolean)
    .join(' · ');

  return (
    <Sheet onDismiss={onDismiss} title={deliverable.name} visible={visible}>
      <View style={{ gap: 4, marginBottom: 10 }}>
        <View style={{ alignItems: 'center', flexDirection: 'row', gap: 5 }}>
          <AppSymbol
            name={deliverable.directory
              ? FILE_ROW_DETAIL_ICON.directory
              : FILE_ROW_DETAIL_ICON.file}
            size={12}
            tintColor={theme.textTertiary}
          />
          <Text numberOfLines={1} style={{ color: theme.textTertiary, flexShrink: 1, fontSize: 12.5 }}>
            {deliverableFileName(deliverable)}
            {host ? ` · ${host}` : ''}
          </Text>
        </View>
        {origin ? (
          <Text style={{ color: theme.textGhost, fontSize: 12 }}>{origin}</Text>
        ) : null}
        <Text
          numberOfLines={2}
          style={{ color: theme.textGhost, fontSize: 11.5 }}
        >
          {deliverable.sourcePath ?? deliverable.path}
        </Text>
      </View>
      <DeliverablePreview
        deliverable={deliverable}
        image={image}
        imageMime={imageMimeForPath(deliverable.path)}
        kind={kind}
        text={text}
      />
    </Sheet>
  );
}

function DeliverablePreview({
  deliverable,
  kind,
  text,
  image,
  imageMime,
}: {
  deliverable: BossDeliverable;
  kind: 'image' | 'markdown' | 'text' | 'directory';
  text: { data?: string; error: unknown; isPending: boolean };
  image: { data?: string; error: unknown; isPending: boolean };
  imageMime: string | null;
}) {
  const theme = useTheme();
  const markdownStyles = useMarkdownStyles();

  if (kind === 'directory') {
    return (
      <PreviewNotice
        detail="Folders aren’t previewable here — this deliverable stays attached to your next message to the boss so it can work with the files directly."
        title={`${deliverable.name} is a folder`}
      />
    );
  }

  if (kind === 'image') {
    if (image.isPending) {return <PreviewLoading />;}
    if (image.error) {
      return (
        <PreviewNotice
          detail={image.error instanceof Error ? image.error.message : String(image.error)}
          title="Couldn’t read this image"
        />
      );
    }

    return (
      <Image
        ios={{ stretch: 'aspectFit' }}
        src={`data:${imageMime};base64,${image.data ?? ''}`}
        style={{ aspectRatio: 1, width: '100%' }}
        web={{ style: { objectFit: 'contain', maxHeight: 320 } }}
      />
    );
  }

  if (text.isPending) {return <PreviewLoading />;}
  if (text.error) {
    return (
      <PreviewNotice
        detail={text.error instanceof Error ? text.error.message : String(text.error)}
        title="No preview for this file"
      />
    );
  }

  const content = text.data ?? '';
  const truncated = content.length > MAX_PREVIEW_CHARACTERS;
  if (kind === 'markdown') {
    const blocks = parseBlocks(content.slice(0, MAX_PREVIEW_CHARACTERS));
    return (
      <View style={{ gap: 8 }}>
        {blocks.map((block) => (
          <MdBlockSettled
            node={block.node}
            source={block.source}
            styles={markdownStyles}
          />
        ))}
        {truncated ? <PreviewTruncated /> : null}
      </View>
    );
  }

  return (
    <View>
      <Text
        style={{
          color: theme.textSecondary,
          fontFamily: MonoFont,
          fontSize: 12.5,
          lineHeight: '18px',
        }}
      >
        {content.slice(0, MAX_PREVIEW_CHARACTERS) || ' '}
      </Text>
      {truncated ? <PreviewTruncated /> : null}
    </View>
  );
}

function PreviewLoading() {
  return (
    <View style={{ alignItems: 'center', paddingVertical: 24 }}>
      <ActivityIndicator busy />
    </View>
  );
}

function PreviewNotice({ title, detail }: { title: string; detail: string }) {
  const theme = useTheme();
  return (
    <View
      style={{
        backgroundColor: theme.surfaceMuted,
        borderColor: theme.separator,
        borderRadius: Radius.medium,
        borderWidth: 1,
        gap: 4,
        padding: 12,
      }}
    >
      <Text style={{ color: theme.text, fontSize: 14, fontWeight: '600' }}>{title}</Text>
      <Text style={{ color: theme.textSecondary, fontSize: 13 }}>{detail}</Text>
    </View>
  );
}

function PreviewTruncated() {
  const theme = useTheme();
  return (
    <Text style={{ color: theme.textTertiary, fontSize: 12, marginTop: 6 }}>
      File truncated in this preview.
    </Text>
  );
}
