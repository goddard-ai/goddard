/** @jsxImportSource @nativescript-community/octane */
import type { CSSProperties } from 'octane/jsx-runtime';
import type { NotificationThread } from '@waku/client';
import { useCallback, useMemo, useState } from 'octane';
import { Spinner, Pressable, ScrollableArea, Text, View } from '@octane-xplat/ui';

import { AppSymbol } from '@/components/app-symbol';
import { NativeTint } from '@/lib/theme-platform';
import { Radius } from '@/lib/theme-colors';
import { useTheme } from '@/lib/use-theme';
import { Haptics } from '@/lib/haptics';
import { showAlert } from '@/lib/alert';
import { openUrl } from '@/lib/open-url';
import {
  daemonKeys,
  listNotifications,
  markAllNotificationsRead,
  markNotificationDone,
  markNotificationRead,
  markRepoNotificationsRead,
} from '@/lib/daemon-api';

import { useDaemon, useDaemonQuery } from '@/lib/daemon-hooks';
import { invalidateDaemonQueries } from '@/lib/daemon-data';
import { relativeSessionTime } from '@/lib/session-presentation';
import { useScreenHeaderInset } from '@/components/screen-header';

const SUBJECT_LABELS: Record<NotificationThread['subjectType'], string> = {
  pullRequest: 'PR',
  issue: 'Issue',
  discussion: 'Discussion',
  release: 'Release',
  commit: 'Commit',
  checkSuite: 'Checks',
  workflowRun: 'Workflow',
  repositoryInvitation: 'Invitation',
  repositoryVulnerabilityAlert: 'Security',
  other: 'Update',
};

const REASON_LABELS: Record<NotificationThread['reason'], string> = {
  assign: 'assigned',
  author: 'author',
  ciActivity: 'CI activity',
  comment: 'comment',
  invitation: 'invitation',
  manual: 'manual',
  mention: 'mention',
  reviewRequested: 'review requested',
  securityAlert: 'security alert',
  stateChange: 'state change',
  subscribed: 'subscribed',
  teamMention: 'team mention',
  other: 'update',
};

interface RepoSection {
  title: string;
  data: NotificationThread[];
}

/** Daemon-wide GitHub inbox — mirrors desktop's notification panel. The
 * ops carry no `cwd`; they read the daemon host's `gh` credential. */
export default function NotificationsScreen() {
  const theme = useTheme();
  const daemon = useDaemon();
  const headerInset = useScreenHeaderInset();
  const profileId = daemon.activeProfile?.id ?? 'disconnected';
  const [showAll, setShowAll] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);

  const inbox = useDaemonQuery(
    daemonKeys.notifications(profileId, showAll),
    () => listNotifications(daemon.client!, showAll),
    { enabled: Boolean(daemon.client && daemon.phase === 'connected') },
  );

  const poll = inbox.data;
  const threads = poll?.status === 'changed' ? poll.threads : [];

  const invalidate = useCallback(() => {
    invalidateDaemonQueries(profileId, (kind) => kind === 'notifications');
  }, [profileId]);

  const runOp = useCallback(
    (label: string, op: () => Promise<void>) => {
      if (busy) return;
      setBusy(label);
      void op()
        .then(() => Haptics.notificationAsync('success'))
        .catch((cause) =>
          void showAlert(
            'Couldn’t update notifications',
            cause instanceof Error ? cause.message : String(cause),
          ),
        )
        .finally(() => {
          setBusy(null);
          invalidate();
        });
    },
    [busy, invalidate],
  );

  const sections = useMemo<RepoSection[]>(() => {
    const groups = new Map<string, NotificationThread[]>();
    for (const thread of threads) {
      const group = groups.get(thread.repo);
      if (group) group.push(thread);
      else groups.set(thread.repo, [thread]);
    }

    return [...groups.entries()].map(([title, data]) => ({ title, data }));
  }, [threads]);

  const open = useCallback((thread: NotificationThread) => {
    const url = thread.url ?? thread.repoUrl;
    try {
      openUrl(url);
    } catch {
      void showAlert('Couldn’t open link', url);
    }
  }, []);

  return (
    <View style={{ flex: '1 1', backgroundColor: theme.background }}>
      <View style={{ height: headerInset }} />
      <View style={{ ...styles.controls, borderBottomColor: theme.border }}>
        <View style={{ ...styles.scopeToggle, backgroundColor: theme.overlay }}>
          {([false, true] as const).map((all) => (
            <Pressable
              accessibilityRole="button"
              accessibilityState={{ selected: showAll === all }}
              onPress={() => setShowAll(all)}
              style={{
                ...styles.scopeOption,
                ...(showAll === all && { backgroundColor: theme.surface }),
              }}
            >
              <Text
                style={{
                  ...styles.scopeLabel,
                  color: showAll === all ? theme.text : theme.textTertiary,
                }}
              >
                {all ? 'All' : 'Unread'}
              </Text>
            </Pressable>
          ))}
        </View>
        <Pressable
          accessibilityLabel="Mark all notifications read"
          accessibilityRole="button"
          disabled={busy != null}
          onPress={() =>
            runOp('all', () => markAllNotificationsRead(daemon.client!))
          }
          style={{ ...styles.textButton, opacity: busy != null ? 0.5 : 1 }}
        >
          <Text style={{ ...styles.textButtonLabel, color: NativeTint as string }}>
            Mark all read
          </Text>
        </Pressable>
      </View>
      {!daemon.client || daemon.phase !== 'connected' ? (
        <InboxMessage
          detail="Connect to a daemon to read its GitHub inbox."
          title="Not connected"
        />
      ) : inbox.isPending ? (
        <View style={styles.message}>
          <Spinner busy />
        </View>
      ) : inbox.error ? (
        <InboxMessage
          detail={
            inbox.error instanceof Error
              ? inbox.error.message
              : String(inbox.error)
          }
          title="Couldn’t load notifications"
        />
      ) : poll?.status === 'unavailable' ? (
        <InboxMessage
          detail={
            poll.availability === 'missingCli'
              ? 'Install the GitHub CLI on the daemon host.'
              : 'Run `gh auth login` on the daemon host.'
          }
          title="GitHub unavailable"
        />
      ) : (
        <ScrollableArea style={styles.list}>
          {sections.length === 0 ? (
            <InboxMessage
              detail={
                showAll
                  ? 'The daemon’s GitHub inbox is empty.'
                  : 'Nothing is unread.'
              }
              title={showAll ? 'No notifications' : 'All caught up'}
            />
          ) : null}
          {sections.map((section) => (
            <View>
              <View
                style={{
                  ...styles.repoHeader,
                  backgroundColor: theme.background,
                  borderBottomColor: theme.border,
                }}
              >
                <Text
                  numberOfLines={1}
                  style={{ ...styles.repoTitle, color: theme.textSecondary }}
                >
                  {section.title}
                </Text>
                <Pressable
                  accessibilityLabel={`Mark ${section.title} read`}
                  accessibilityRole="button"
                  onPress={() =>
                    runOp(`repo:${section.title}`, () =>
                      markRepoNotificationsRead(daemon.client!, section.title),
                    )
                  }
                  style={styles.textButton}
                >
                  <Text style={{ ...styles.repoAction, color: NativeTint as string }}>
                    Mark read
                  </Text>
                </Pressable>
              </View>
              {section.data.map((item) => (
                <Pressable
                  accessibilityHint="Opens the thread on GitHub"
                  accessibilityRole="button"
                  onPress={() => open(item)}
                  style={{ ...styles.row, borderBottomColor: theme.border }}
                >
                  <View
                    style={{
                      ...styles.unreadDot,
                      backgroundColor: item.unread ? theme.accent : 'transparent',
                    }}
                  />
                  <View style={styles.rowCopy}>
                    <Text
                      numberOfLines={2}
                      style={{ ...styles.rowTitle, color: theme.text }}
                    >
                      {item.title}
                    </Text>
                    <Text style={{ ...styles.rowMeta, color: theme.textTertiary }}>
                      {[
                        SUBJECT_LABELS[item.subjectType],
                        item.number != null ? `#${item.number}` : null,
                        REASON_LABELS[item.reason],
                        relativeSessionTime(item.updatedAt),
                      ]
                        .filter(Boolean)
                        .join(' · ')}
                    </Text>
                  </View>
                  {item.unread ? (
                    <Pressable
                      accessibilityLabel={`Mark ${item.title} read`}
                      accessibilityRole="button"
                      onPress={() =>
                        runOp(`read:${item.id}`, () =>
                          markNotificationRead(daemon.client!, item.id),
                        )
                      }
                      style={styles.iconButton}
                    >
                      <AppSymbol
                        name={{
                          ios: 'envelope.open',
                          android: 'mark_email_read',
                          web: 'mark_email_read',
                        }}
                        size={17}
                        tintColor={theme.textSecondary}
                      />
                    </Pressable>
                  ) : null}
                  <Pressable
                    accessibilityLabel={`Mark ${item.title} done`}
                    accessibilityRole="button"
                    onPress={() =>
                      runOp(`done:${item.id}`, () =>
                        markNotificationDone(daemon.client!, item.id),
                      )
                    }
                    style={styles.iconButton}
                  >
                    <AppSymbol
                      name={{
                        ios: 'checkmark.circle',
                        android: 'check_circle',
                        web: 'check_circle',
                      }}
                      size={17}
                      tintColor={theme.textSecondary}
                    />
                  </Pressable>
                </Pressable>
              ))}
            </View>
          ))}
        </ScrollableArea>
      )}
    </View>
  );
}

function InboxMessage({ title, detail }: { title: string; detail: string }) {
  const theme = useTheme();
  return (
    <View style={styles.message}>
      <Text style={{ ...styles.messageTitle, color: theme.text }}>{title}</Text>
      <Text style={{ ...styles.messageDetail, color: theme.textTertiary }}>
        {detail}
      </Text>
    </View>
  );
}

const hairline = 0.5;

const styles = {
  controls: {
    alignItems: 'center',
    borderBottomWidth: hairline,
    flexDirection: 'row',
    justifyContent: 'space-between',
    minHeight: 48,
    paddingLeft: 14, paddingRight: 14,
  },
  scopeToggle: {
    borderRadius: Radius.small,
    flexDirection: 'row',
    padding: 2,
  },
  scopeOption: {
    borderRadius: Radius.small - 2,
    minHeight: 28,
    justifyContent: 'center',
    paddingLeft: 12, paddingRight: 12,
  },
  scopeLabel: { fontSize: 13.5, fontWeight: '500' },
  textButton: { justifyContent: 'center', minHeight: 36, paddingLeft: 4, paddingRight: 4 },
  textButtonLabel: { fontSize: 14.5, fontWeight: '500' },
  list: { flex: '1 1' },
  repoHeader: {
    alignItems: 'center',
    borderBottomWidth: hairline,
    flexDirection: 'row',
    justifyContent: 'space-between',
    paddingLeft: 14, paddingRight: 14,
    paddingTop: 7, paddingBottom: 7,
  },
  repoTitle: { flex: '1 1', fontSize: 12.5, fontWeight: '600', marginRight: 10 },
  repoAction: { fontSize: 13, fontWeight: '500' },
  row: {
    alignItems: 'center',
    borderBottomWidth: hairline,
    flexDirection: 'row',
    gap: 10,
    minHeight: 54,
    paddingLeft: 14, paddingRight: 14,
    paddingTop: 9, paddingBottom: 9,
  },
  unreadDot: { borderRadius: 4, height: 8, width: 8 },
  rowCopy: { flex: '1 1', minWidth: 0 },
  rowTitle: { fontSize: 14.5 },
  rowMeta: { fontSize: 12, marginTop: 2 },
  iconButton: {
    alignItems: 'center',
    justifyContent: 'center',
    minHeight: 36,
    minWidth: 30,
  },
  message: {
    alignItems: 'center',
    flex: '1 1',
    gap: 8,
    justifyContent: 'center',
    paddingLeft: 32, paddingRight: 32,
  },
  messageTitle: { fontSize: 16, fontWeight: '600' },
  messageDetail: { fontSize: 13.5, textAlign: 'center' },
} satisfies Record<string, CSSProperties>;
