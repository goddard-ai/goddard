import type {
  AgentSession,
  BossEmployee,
  BossState,
  CustomCommandIcon,
  EmployeeLifecycle,
  MessageAttachment,
  PlanOutcome,
} from '@waku/client';

import { bossEmployeeLifecycle } from '@waku/client';

export type SessionStatusLike = Pick<AgentSession, 'status'>;

/** Desktop's roster split: planning rows and live employees are always
 * visible; finished and expired employees sit behind the "Show more" fold.
 * `dispatching` and `finishing` are still live — neither has reached the
 * user-visible finished state. */
export interface BossRosterSplit {
  live: BossEmployee[];
  finished: BossEmployee[];
}

const LIVE_LIFECYCLES: ReadonlySet<EmployeeLifecycle> = new Set([
  'queued',
  'dispatching',
  'working',
  'finishing',
]);

export function bossEmployeeIsLive(employee: BossEmployee): boolean {
  return LIVE_LIFECYCLES.has(bossEmployeeLifecycle(employee));
}

/** Sidebar employee order: newest summon first. `createdAt` is the summon
 * stamp; records predating it fall back to the session's creation time
 * when the caller can supply one. Sorting on anything mutable — session
 * activity, state — would shuffle a row on every state revision. */
export function sortBossEmployees(
  employees: BossEmployee[],
  sessionCreatedAt?: Map<string, number>,
): BossEmployee[] {
  return [...employees].sort(
    (a, b) =>
      (b.createdAt ?? sessionCreatedAt?.get(b.sessionId) ?? 0)
      - (a.createdAt ?? sessionCreatedAt?.get(a.sessionId) ?? 0),
  );
}

export function bossRosterSplit(
  state: BossState,
  sessionCreatedAt?: Map<string, number>,
): BossRosterSplit {
  const sorted = sortBossEmployees(state.employees, sessionCreatedAt);
  return {
    live: sorted.filter(bossEmployeeIsLive),
    finished: sorted.filter((employee) => !bossEmployeeIsLive(employee)),
  };
}

/** The employee record a session belongs to — live roster entries first,
 * then retired records so an expired-still-open task still resolves. */
export function bossEmployeeForSession(
  state: BossState | null | undefined,
  sessionId: string | undefined,
): BossEmployee | undefined {
  if (!state || !sessionId) {return undefined;}
  return (
    state.employees.find((employee) => employee.sessionId === sessionId)
    ?? state.retiredEmployees.find((employee) => employee.sessionId === sessionId)
  );
}

/** 1-based admission order among queued employees — the daemon's scheduler
 * is strict FIFO on `ticket.sequence`. Never displayed as a number; rank
 * only decides between "Waiting for admission" and "Waiting for earlier
 * work" when the ticket carries no blocker. */
export function bossQueueRanks(state: BossState): Map<string, number> {
  const queued = state.employees
    .filter((employee) => bossEmployeeLifecycle(employee) === 'queued')
    .sort(
      (a, b) =>
        (a.ticket?.sequence ?? Number.MAX_SAFE_INTEGER)
        - (b.ticket?.sequence ?? Number.MAX_SAFE_INTEGER),
    );

  return new Map(queued.map((employee, index) => [employee.sessionId, index + 1]));
}

/** A queued employee's wait reason: the ticket's admission blocker when
 * the scheduler recorded one, else a neutral admission label. Mirrors the
 * desktop's `boss_queue_detail` — non-queued employees return `null`. */
export function bossQueueDetail(
  employee: BossEmployee,
  rank: number | undefined,
): string | null {
  if (bossEmployeeLifecycle(employee) !== 'queued') {return null;}
  const blocker = employee.ticket?.blockedBy?.[0];
  if (blocker) {
    if (blocker.kind === 'modelLimit') {
      return `Waiting for ${employee.ticket?.model ?? 'model'} · ${blocker.used}/${blocker.limit} slots`;
    }

    return blocker.detail;
  }

  return rank === 1 ? 'Waiting for admission' : 'Waiting for earlier work';
}

export interface EmployeeStatusLabel {
  label: string;
  /** Semantic tone the row renders in — matches the desktop's color picks. */
  tone: 'neutral' | 'secondary' | 'warning' | 'danger' | 'working' | 'waiting';
}

/** The roster row's readable status — lifecycle first, then the live
 * session's verdict. Mirrors the desktop's `boss_employee_status_label`:
 * finished, failed, cancelled, and expired stay distinct because expiry
 * describes the roster record, not the outcome. */
export function bossEmployeeStatusLabel(
  employee: BossEmployee,
  session: SessionStatusLike | undefined,
  queueDetail?: string | null,
): EmployeeStatusLabel {
  const lifecycle = bossEmployeeLifecycle(employee);
  switch (lifecycle) {
    case 'queued':
      return {
        label: `Queued · ${queueDetail ?? 'Waiting for admission'}`,
        tone: 'secondary',
      };
    case 'dispatching':
      return { label: 'Starting', tone: 'secondary' };
    case 'finishing':
      return { label: 'Finishing', tone: 'secondary' };
    case 'working': {
      if (employee.blocker != null) {return { label: 'Needs attention', tone: 'warning' };}
      const status = session?.status;
      if (status === 'working' || status === 'connecting') {return { label: 'Working', tone: 'working' };}
      if (status === 'waiting') {return { label: 'Waiting', tone: 'waiting' };}
      if (status === 'background') {return { label: 'Background work', tone: 'secondary' };}
      if (status === 'failed') {return { label: 'Failed', tone: 'danger' };}
      if (status === 'idle') {return { label: 'Idle', tone: 'secondary' };}
      return { label: 'Starting', tone: 'secondary' };
    }
    case 'expired': {
      if (employee.cancelled) {return { label: 'Cancelled', tone: 'secondary' };}
      if (session?.status === 'failed') {return { label: 'Failed', tone: 'danger' };}
      if (employee.blocker != null) {return { label: 'Needs attention', tone: 'warning' };}
      if (!session) {return { label: 'Expired', tone: 'secondary' };}
      return { label: 'Finished', tone: 'secondary' };
    }
  }
}

/** The icon an employee's rows show: the summon-set icon first, then its
 * persona's — matching the desktop's `employee_icon`. */
export function bossEmployeeIcon(
  state: BossState,
  employee: BossEmployee,
): CustomCommandIcon | undefined {
  if (employee.icon) {return employee.icon;}
  return state.personas.find((persona) => persona.id === employee.personaId)?.icon ?? undefined;
}

const JOB_TITLE_FALLBACK_SYMBOL = 'brain';

/** The desktop's `JOB_TITLE_ICON_CATEGORIES` mapped onto SF Symbols — same
 * keywords, same precedence order (specialized work ahead of generic
 * implementation). Whole-word, case-insensitive matching. */
const JOB_TITLE_SYMBOL_CATEGORIES: ReadonlyArray<readonly [readonly string[], string]> = [
  [['bug', 'fix', 'bugfix', 'debug'], 'ladybug'],
  [['translate', 'translation', 'localize', 'localization', 'l10n', 'i18n'], 'character.bubble'],
  [['dependency', 'dependencies', 'package', 'packaging', 'publish', 'publishing'], 'shippingbox'],
  [['infrastructure', 'infra', 'deploy', 'deployment', 'daemon', 'server'], 'server.rack'],
  [['database', 'databases', 'sql', 'query', 'queries', 'schema', 'schemas'], 'cylinder'],
  [['implement', 'build', 'migrate', 'refactor'], 'terminal'],
  [['investigate'], 'magnifyingglass'],
  [['verify', 'validate', 'smoke'], 'checkmark.circle'],
  [['test', 'qa'], 'flask'],
  [['integrate', 'merge', 'land', 'release'], 'arrow.triangle.merge'],
  [['review', 'audit'], 'eye'],
  [['design', 'spec'], 'pencil'],
  [['research', 'analyze'], 'folder.badge.magnifyingglass'],
  [['write', 'docs'], 'doc.text'],
  [['security', 'privacy'], 'lock'],
  [['measure', 'benchmark', 'perf'], 'gauge'],
  [['port'], 'arrow.triangle.branch'],
];

// NativeScript's V8 is built without ICU — \p{…} escapes are a parse error
// there, so the literal form can't appear in the bundle. Build at runtime
// and fall back to ASCII on engines without Unicode property support.
const NON_WORD = (() => {
  try {
    return new RegExp('[^\\p{L}\\p{N}]+', 'u');
  } catch {
    return /[^A-Za-z0-9]+/;
  }
})();

/** The SF Symbol a job title earns when no explicit icon was chosen:
 * the first category containing one of the title's complete words, or
 * the brain fallback. */
export function jobTitleSymbol(title: string): string {
  const words = title
    .split(NON_WORD)
    .filter(Boolean)
    .map((word) => word.toLowerCase());

  for (const [keywords, symbol] of JOB_TITLE_SYMBOL_CATEGORIES) {
    if (keywords.some((keyword) => words.includes(keyword))) {return symbol;}
  }

  return JOB_TITLE_FALLBACK_SYMBOL;
}

/** `CustomCommandIcon` → SF Symbol, covering the same set the daemon can
 * stamp on an employee or persona record. */
export function commandIconSymbol(icon: CustomCommandIcon): string {
  return (
    {
      terminal: 'terminal',
      command: 'command',
      zap: 'bolt',
      wrench: 'wrench',
      gauge: 'gauge',
      package: 'shippingbox',
      git_branch: 'arrow.triangle.branch',
      git_hub: 'arrow.triangle.branch',
      folder: 'folder',
      file: 'doc',
      search: 'magnifyingglass',
      globe: 'globe',
      server: 'server.rack',
      cloud_upload: 'icloud.and.arrow.up',
      download: 'arrow.down.circle',
      bot: 'desktopcomputer',
      sparkle: 'sparkles',
      star: 'star',
      target: 'target',
      queue: 'list.bullet',
      compose: 'square.and.pencil',
      chart: 'chart.bar',
      refresh: 'arrow.clockwise',
      archive: 'archivebox',
      bug: 'ladybug',
      languages: 'character.bubble',
      database: 'cylinder',
      circle_check: 'checkmark.circle',
      beaker: 'flask',
      git_merge: 'arrow.triangle.merge',
      eye: 'eye',
      pencil: 'pencil',
      folder_search: 'folder.badge.magnifyingglass',
      file_text: 'doc.text',
      lock: 'lock',
      fork: 'arrow.triangle.branch',
      brain: 'brain',
    } satisfies Record<CustomCommandIcon, string>
  )[icon];
}

/** The symbol an employee row leads with: the employee/persona icon when
 * the summon named one, else the job-title classifier. */
export function bossEmployeeSymbol(state: BossState, employee: BossEmployee): string {
  const icon = bossEmployeeIcon(state, employee);
  return icon ? commandIconSymbol(icon) : jobTitleSymbol(employee.jobTitle);
}

/** The session-reference attachment a supervision prompt carries — the
 * viewed employee's task reference, which `attachmentPromptToken` splices
 * into the provider-facing prompt as `[session "name" (task_id: …)]`. */
export function sessionReferenceAttachment(
  sessionId: string,
  name: string,
): MessageAttachment {
  return {
    path: '',
    mention: `[session "${name}" (task_id: ${sessionId})]`,
    name,
    is_dir: false,
    is_image: false,
    blob_reference: null,
    pasted_text_preview: null,
    session_id: sessionId,
  };
}

/** The assignment disclosure's label/value inventory — Brief, Persona,
 * Model, Workspace, Access, Resources, mirroring the desktop's
 * `employee_assignment_rows`. */
export function employeeAssignmentRows(
  state: BossState,
  session: AgentSession | undefined,
  employee: BossEmployee,
): Array<{ label: string; value: string }> {
  const rows: Array<{ label: string; value: string }> = [];
  const brief = employee.ticket?.prompt.trim();
  if (brief) {rows.push({ label: 'Brief', value: brief });}

  const persona = state.personas.find((item) => item.id === employee.personaId);
  if (persona) {rows.push({ label: 'Persona', value: persona.name });}

  rows.push({ label: 'Model', value: session?.model ?? employee.ticket?.model ?? 'Default' });

  rows.push({ label: 'Workspace', value: employeeWorkspaceLabel(session, employee) });

  const access: string[] = [];
  const permissions = employee.permissions;
  if (permissions.bucketIds.length) {access.push(`Memory buckets: ${permissions.bucketIds.length}`);}
  if (permissions.integrationIds.length) {access.push(`Integrations: ${permissions.integrationIds.length}`);}
  if (permissions.summonEmployees) {access.push('May summon employees');}
  if (permissions.computerUse) {access.push('Computer use');}
  rows.push({
    label: 'Access',
    value: access.length ? access.join(' · ') : 'None beyond task defaults',
  });

  if (employee.ticket) {
    const resources = employee.ticket.resources;
    const empty =
      resources.exclusive.length === 0
      && resources.resident_devices === 0
      && resources.native_builds === 0
      && resources.desktop_input === 0;

    if (empty) {
      rows.push({ label: 'Resources', value: 'Model slot only' });
    } else {
      const parts: string[] = [];
      if (resources.exclusive.length) {parts.push(`exclusive: ${resources.exclusive.length}`);}
      if (resources.resident_devices) {parts.push(`devices: ${resources.resident_devices}`);}
      if (resources.native_builds) {parts.push(`builds: ${resources.native_builds}`);}
      if (resources.desktop_input) {parts.push('desktop input');}
      const detail = parts.join(' · ');
      rows.push({
        label: 'Resources',
        value: employee.ticket.reservation ? `Reserved: ${detail}` : `Requested: ${detail}`,
      });
    }
  }

  return rows;
}

function employeeWorkspaceLabel(
  session: AgentSession | undefined,
  employee: BossEmployee,
): string {
  const workspace = session?.workspace;
  if (workspace?.kind === 'worktree') {
    return workspace.name?.trim() ? workspace.name : workspace.path;
  }

  if (workspace?.kind === 'newWorktree') {return 'New worktree';}
  if (employee.ticket?.workspace === 'worktree') {return 'New worktree';}
  if (employee.ticket?.workspace === 'adopt' && employee.ticket.adoptWorktree) {
    return employee.ticket.adoptWorktree;
  }

  return 'Project checkout';
}

/** Open blockers the boss surface keeps actionable — roster rows and the
 * boss chat's notice both read this so the two stay consistent. */
export function bossOpenBlockers(state: BossState): BossEmployee[] {
  return state.employees.filter((employee) => employee.blocker != null);
}

export interface BossModelCapacity {
  provider: string;
  model: string;
  used: number;
  liveLimit: number;
  hardCap: number;
}

/** Per-model slot usage for the read-only Capacity view: `used` counts
 * employees holding a claim — dispatching, working, and finishing, the
 * states where the reservation is still outstanding. Queued tickets have
 * no claim yet; released claims left with expiry. */
export function bossModelCapacity(state: BossState): BossModelCapacity[] {
  const holding = state.employees.filter((employee) => {
    const lifecycle = bossEmployeeLifecycle(employee);
    return lifecycle === 'dispatching' || lifecycle === 'working' || lifecycle === 'finishing';
  });

  const used = (provider: string, model: string) =>
    holding.filter(
      (employee) => employee.ticket?.provider === provider && employee.ticket.model === model,
    ).length;

  const limits = new Map<string, BossModelCapacity>();
  for (const limit of state.resourcePolicy.modelLimits) {
    limits.set(`${limit.provider}/${limit.model}`, {
      provider: limit.provider,
      model: limit.model,
      used: used(limit.provider, limit.model),
      liveLimit: limit.liveLimit,
      hardCap: limit.hardCap,
    });
  }

  // Uncapped models still show their live usage — the policy's silence
  // means "no limit", not "invisible".
  for (const employee of holding) {
    const ticket = employee.ticket;
    if (!ticket) {continue;}
    const key = `${ticket.provider}/${ticket.model}`;
    if (!limits.has(key)) {
      limits.set(key, {
        provider: ticket.provider,
        model: ticket.model,
        used: used(ticket.provider, ticket.model),
        liveLimit: -1,
        hardCap: -1,
      });
    }
  }

  return [...limits.values()].sort(
    (a, b) => a.provider.localeCompare(b.provider) || a.model.localeCompare(b.model),
  );
}

export function planOutcomeLabel(outcome: PlanOutcome | null | undefined): string | null {
  if (outcome === 'completed') {return 'Completed';}
  if (outcome === 'abandoned') {return 'Abandoned';}
  return null;
}
