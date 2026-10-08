import type { AgentSession, BossPlan } from '@waku/client';
import { useEffect, useRef, useState } from 'octane';
import { Pressable, pushRoute, Text, useSafeAreaInsets, View } from '@octane-xplat/ui';
import { bossReadPlanDocument, bossRequest } from '@waku/client/boss';
import { Sheet } from '@/components/sheet';
import { MdBlockSettled, useMarkdownStyles } from '@/components/md-block-row';
import { MobileComposer, PermissionPanel } from '@/components/mobile-composer';
import { useDaemon, useDaemonQuery } from '@/lib/daemon-hooks';
import { useBossState, useTaskState } from '@/lib/daemon-queries';
import { useRuntime } from '@/lib/use-runtime';
import { useTheme } from '@/lib/use-theme';
import { showAlert } from '@/lib/alert';
import { planApprovalSent, planFilter } from '@/lib/plans';
import { relativeSessionTime } from '@/lib/session-presentation';
import { parseBlocks } from '@/md/parse';

export function usePlanDocument(planFile?: string) {
  const daemon = useDaemon();
  const boss = useBossState();
  const enabled = daemon.phase === 'connected' && Boolean(daemon.client && planFile);
  const query = useDaemonQuery(
    ['daemon', daemon.activeProfile?.id ?? 'disconnected', 'plan-document', planFile ?? ''],
    () => bossReadPlanDocument(daemon.client!, planFile!),
    { enabled, staleTime: 1_000 },
  );

  // File writes bump the Boss revision. Keep the same cache entry while
  // re-reading so rewrites do not remove the Plan entry or blank its sheet.
  useEffect(() => {
    if (enabled) {void query.refetch();}
  }, [enabled, planFile, daemon.activeProfile?.id, boss.data?.revision]);

  return query;
}

export function PlanDocument({ content }: { content: string }) {
  const styles = useMarkdownStyles();
  return <View style={{ gap: 8 }}>{parseBlocks(content).map((block) => (
    <MdBlockSettled node={block.node} source={block.source} styles={styles} />
  ))}</View>;
}

export function PlanDetail({ plan }: { plan: BossPlan }) {
  const theme = useTheme();
  const planDoc = usePlanDocument(plan.planFile);
  const session = useTaskState().data?.sessions.find((entry) => entry.id === plan.sessionId);
  const state = planFilter(plan, session);
  const age = plan.finalizedAt != null ? relativeSessionTime(plan.finalizedAt) : null;
  return <View style={{ gap: 12, padding: 14 }}>
    <Text style={{ color: theme.text, fontSize: 18, fontWeight: '600' }}>{plan.idea}</Text>
    <Text style={{ color: theme.textSecondary }}>{state === 'approved'
      ? `Finalized ${age === 'just now' ? age : age + ' ago'}`
      : state === 'active' ? 'In discussion' : 'Archived'}</Text>
    {session && session.archived_at == null ? <Pressable accessibilityRole="button" onPress={() => pushRoute({ stack: 'root', name: 'session/:id', params: { id: plan.sessionId } })}>
      <Text style={{ color: theme.accent }}>Open discussion</Text>
    </Pressable> : null}
    {planDoc.error ? <Text style={{ color: theme.textSecondary }}>Couldn’t load plan: {String(planDoc.error)}</Text>
      : planDoc.data === undefined ? <Text style={{ color: theme.textSecondary }}>Loading plan…</Text>
      : <PlanDocument content={planDoc.data} />}
  </View>;
}

/** Mobile Plan tab: a persistent entry one tap from the discussion. Closing
 * the sheet never removes the entry. Selection annotations are deferred to v1.1. */
export function PlanningComposer({ session, onSubmitted }: { key?: string; session: AgentSession; onSubmitted?: () => void }) {
  const theme = useTheme();
  const daemon = useDaemon();
  const runtime = useRuntime();
  const insets = useSafeAreaInsets();
  const boss = useBossState();
  const planDoc = usePlanDocument(session.planning?.planFile);
  const [open, setOpen] = useState(false);
  const [finalizing, setFinalizing] = useState(false);
  const [approved, setApproved] = useState(false);
  const dispatching = useRef(false);
  const manual = useRef(false);
  const navigated = useRef(false);
  const bossPlan = boss.data?.planning.find((plan) => plan.sessionId === session.id);
  const finalized = approved || session.planning?.finalizedAt != null || bossPlan?.finalizedAt != null;

  const observedDraft = useRef(false);
  const [hasDocument, setHasDocument] = useState(Boolean(planDoc.data?.trim()));
  useEffect(() => {
    if (planDoc.data?.trim()) {setHasDocument(true);}
  }, [planDoc.data]);

  const sealed = finalized || finalizing || planApprovalSent(session);
  useEffect(() => {
    // Loading an already-approved library discussion is not a finalization
    // event. Navigate only after this mounted view observed its live draft.
    if (!finalized && bossPlan) {observedDraft.current = true;}
    if (finalized && observedDraft.current && !manual.current && !navigated.current && boss.data?.sessionId) {
      navigated.current = true;
      pushRoute({ stack: 'root', name: 'session/:id', params: { id: boss.data.sessionId } });
    }
  }, [finalized, bossPlan, boss.data?.sessionId]);

  const finalize = () => {
    if (!daemon.client || sealed || dispatching.current || !session.planning) {return;}
    dispatching.current = true;
    manual.current = true;
    setFinalizing(true);
    void bossRequest(daemon.client, { type: 'finalizePlan', planFile: session.planning.planFile })
      .then((result) => {
        if (result.type !== 'planFinalized') {throw new Error('Unexpected finalization response');}
        setApproved(true);
        void boss.refetch();
      }).catch((cause) => {
        dispatching.current = false;
        setFinalizing(false);
        manual.current = false;
        void showAlert('Couldn’t finalize plan', cause instanceof Error ? cause.message : String(cause));
      });
  };

  const permission = runtime.permissions[session.id];
  const respond = (optionId: string) => {
    if (!permission) {return Promise.resolve();}
    const approving = optionId === 'finalize' && (permission.titleI18n?.key === 'boss.plan_finalize_title' || permission.title === 'Finalize plan?');
    if (approving) {setFinalizing(true);}
    return runtime.respond(session.id, permission.requestId, optionId).catch((cause) => {
      if (approving) {
        setFinalizing(false);
        void showAlert('Couldn’t finalize plan', cause instanceof Error ? cause.message : String(cause));
      }

      throw cause;
    });
  };

  return <View>
    {hasDocument ? <Pressable accessibilityRole="button" accessibilityLabel="Open Plan tab" onPress={() => setOpen(true)} style={{ padding: 12 }}>
      <Text style={{ color: theme.accent }}>Plan</Text>
    </Pressable> : null}
    {!sealed ? <Pressable accessibilityRole="button" accessibilityLabel="Finalize plan" disabled={daemon.phase !== 'connected'} onPress={finalize} style={{ padding: 12 }}>
      <Text style={{ color: theme.accent }}>Finalize plan</Text>
    </Pressable> : null}
    {sealed ? <View style={{ padding: 14, paddingBottom: Math.max(insets.bottom, 14), gap: 4 }}>
      <Text style={{ color: theme.textSecondary }}>{finalized ? 'Plan finalized' : 'Handing the design to the boss'}</Text>
      <Text style={{ color: theme.textTertiary }}>{finalized
        ? 'The design doc is frozen — the boss runs implementation; this session only answers questions.'
        : 'This session no longer takes prompts.'}</Text>
      {permission && !finalizing ? <PermissionPanel permission={permission} onRespond={respond} /> : null}
    </View> : <MobileComposer session={session} onSubmitted={onSubmitted} onPermissionRespond={respond} />}
    <Sheet title="Plan" visible={open} onDismiss={() => setOpen(false)}>
      {planDoc.error ? <Text style={{ color: theme.danger }}>Couldn’t load plan: {String(planDoc.error)}</Text> : null}
      <PlanDocument content={planDoc.data ?? ''} />
      <Text style={{ color: theme.textTertiary, paddingVertical: 12 }}>Selection comments are coming in v1.1. Discuss changes in the planning chat; the approved document stays frozen.</Text>
    </Sheet>
  </View>;
}
