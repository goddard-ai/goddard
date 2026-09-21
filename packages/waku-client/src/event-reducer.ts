import type {
  ActivityItem,
  ActivityKind,
  AgentSession,
  ProviderResumeCursor,
  QueuedMessage,
  ReportedCommand,
  SequencedEvent,
  ThreadGoal,
  TranscriptBlock,
  TurnStatus,
  WireTranslation,
} from './generated'

/** A queued follow-up the daemon parked from an agent prompt or automation
 * run. The daemon owns its delivery and removal — clients must not submit,
 * edit, or locally delete it. Older documents have no `source`; those are
 * all composer-owned. */
export function isAgentQueuedMessage(message: QueuedMessage): boolean {
  return typeof message.source === 'object' && message.source !== null && 'agent' in message.source
}

/** Replace the daemon-owned slice of the follow-up queue with the daemon's
 * latest snapshot, keeping composer-queued entries; combined order follows
 * `created_at`. Mirrors `AgentSession::merge_agent_queued`. */
export function mergeAgentQueuedMessages(
  queuedMessages: QueuedMessage[] | undefined,
  agentMessages: QueuedMessage[],
): QueuedMessage[] {
  const combined = [
    ...(queuedMessages ?? []).filter((message) => !isAgentQueuedMessage(message)),
    ...agentMessages,
  ]
  combined.sort((a, b) => a.created_at - b.created_at)
  return combined
}

export interface PendingPermission {
  requestId: string
  title: string
  detail: string
  /** The i18n semantics behind daemon-composed title/detail; render through
   * the client's translator when present, else the fallback strings. */
  titleI18n?: WireTranslation
  detailI18n?: WireTranslation
  options: Array<{ id: string; label: string; labelI18n?: WireTranslation; allow: boolean }>
}

export interface PendingUserInput {
  requestId: string
  questions: Array<{
    id: string
    header: string
    question: string
    options: Array<{ label: string; description?: string }>
    multiSelect: boolean
  }>
}

export interface RuntimeEventResult {
  session: AgentSession
  permission?: PendingPermission | null
  userInput?: PendingUserInput | null
  settled: boolean
  removeRuntime: boolean
  error?: string
  /** The i18n semantic behind `error`, when the daemon composed it from a
   * known key rather than relaying provider text. */
  errorI18n?: WireTranslation
}

export interface ReducerClock {
  nowSeconds: () => number
  nowMillis: () => number
  randomUUID: () => string
}

const defaultClock: ReducerClock = {
  nowSeconds: () => Math.floor(Date.now() / 1_000),
  nowMillis: () => Date.now(),
  randomUUID: () => crypto.randomUUID(),
}

/** What an `error`/`localizedError` does to the open turn: unwinds an
 * optimistic pursuit with no submission, else fails the active turn and
 * stores the message as its assistant-visible reason. */
function failTurn(session: AgentSession, message: string, clock: ReducerClock) {
  // An optimistic pursuit turn has no submission to fail with. Unwind it
  // so the error cannot strand a spinner; if the pursuit does start
  // later, its own start report recreates the turn.
  const pursuit = session.turns.at(-1)
  if (
    pursuit && pursuit.status === 'running'
    && !pursuit.provider_turn_started
    && !session.messages.some((message) => message.turn_id === pursuit.id)
  ) {
    session.turns.pop()
    if (['connecting', 'working', 'waiting', 'background'].includes(session.status)) {
      session.status = 'idle'
    }
    return
  }
  const turn = activeTurn(session)
  if (!turn || session.status === 'working') return
  const hasAssistant = session.messages.some(
    (message) => message.turn_id === turn.id && message.role === 'assistant',
  )
  session.status = 'failed'
  if (!hasAssistant) {
    session.messages.push({
      id: clock.randomUUID(),
      turn_id: turn.id,
      role: 'assistant',
      content: message,
      created_at: clock.nowSeconds(),
      streaming: false,
    })
  }
}

function asWireTranslation(value: unknown): WireTranslation | undefined {
  const record = asRecord(value)
  if (!record || typeof record.key !== 'string') return undefined
  const args = asRecord(record.args)
  return {
    key: record.key,
    args: args
      ? Object.fromEntries(
          Object.entries(args).filter((entry): entry is [string, string] => typeof entry[1] === 'string'),
        )
      : undefined,
  }
}

export function reduceRuntimeEvent(
  current: AgentSession,
  wire: SequencedEvent,
  clock: ReducerClock = defaultClock,
  processExitError: string | null = null,
): RuntimeEventResult {
  const session = clone(current)
  const { kind, payload } = wire.event
  const result: RuntimeEventResult = {
    session,
    settled: false,
    removeRuntime: false,
  }

  session.runtime_event_cursor = {
    runtime_id: wire.runtimeId,
    epoch: wire.epoch,
    sequence: wire.sequence,
  }

  switch (kind) {
    case 'connected':
      session.provider_cursor = (payload as ProviderResumeCursor | null) ?? null
      if (
        session.provider_cursor?.provider === 'claude'
          && session.provider_cursor.resumeAt
      ) {
        const turn = activeTurn(session)
        if (turn) turn.provider_resume_at = session.provider_cursor.resumeAt
      }
      if (session.status === 'connecting') session.status = 'working'
      break
    case 'agentPresetSelected':
      session.agent_preset = typeof payload === 'string' ? payload : null
      break
    case 'autoTitleUpdated': {
      const title = typeof payload === 'string' ? stripInjectedPromptBlocks(payload) : ''
      session.auto_title = title || null
      break
    }
    case 'availableCommands':
      if (Array.isArray(payload)) session.available_commands = payload as ReportedCommand[]
      break
    case 'promptSubmitted': {
      // A prompt reached this runtime — from another client, or the echo of
      // this one. Mirror the desktop's `adopt_submitted_prompt`, reusing the
      // submitter's ids so every client's projection names the same rows.
      const value = asRecord(payload)
      if (!value || typeof value.message !== 'string') break
      adoptSubmittedPrompt(
        session,
        value.message,
        typeof value.turnId === 'string' ? value.turnId : clock.randomUUID(),
        typeof value.messageId === 'string' ? value.messageId : clock.randomUUID(),
        typeof value.sentByTask === 'string' ? value.sentByTask : null,
        value.hidden === true,
        clock,
      )
      break
    }
    case 'queuedMessagesChanged': {
      // The daemon owns the agent-sourced slice of the follow-up queue — a
      // parked prompt appeared, delivered, or was cancelled. Composer-queued
      // entries pass through untouched.
      const value = asRecord(payload)
      if (!value || !Array.isArray(value.messages)) break
      session.queued_messages = mergeAgentQueuedMessages(
        session.queued_messages,
        value.messages as QueuedMessage[],
      )
      session.updated_at = clock.nowSeconds()
      break
    }
    case 'turnStarted': {
      const turn = activeTurn(session)
      if (turn) {
        turn.provider_turn_started = true
        session.status = 'working'
      } else if (
        (session.provider === 'codex' || session.provider === 'claude')
        && !['connecting', 'working', 'waiting', 'background'].includes(session.status)
      ) {
        // Some providers start turns on their own: Codex goal continuation
        // pursues an active goal whenever the thread is idle, and Claude Code
        // re-enters the model once a backgrounded command, subagent or monitor
        // settles. Give the turn a transcript home — there is no user message
        // for it — so its work streams in instead of being dropped.
        session.turns.push({
          id: clock.randomUUID(),
          turn_count: session.turns.length + 1,
          status: 'running',
          provider_turn_started: true,
          provider_resume_at: null,
          started_at: clock.nowSeconds(),
          completed_at: null,
          checkpoint: null,
        })
        session.status = 'working'
      }
      break
    }
    case 'turnParked': {
      // The provider's reply ended while detached work it will wake the
      // session for still runs. The turn stays open for that wake; only the
      // streaming state settles.
      if (!activeTurn(session)) break
      finishStreamingMessages(session)
      completeActivities(session)
      session.status = 'background'
      break
    }
    case 'textDelta':
      if (typeof payload === 'string' && acceptsTurnOutput(session)) {
        appendText(session, payload, clock)
      }
      break
    case 'reasoningDelta':
      if (typeof payload === 'string' && acceptsTurnOutput(session)) {
        appendReasoning(session, payload, clock)
      }
      break
    case 'activity': {
      const value = asRecord(payload)
      if (!acceptsTurnOutput(session) || !value || typeof value.title !== 'string') break
      upsertActivity(
        session,
        {
          id: clock.randomUUID(),
          source_id: typeof value.id === 'string' ? value.id : null,
          kind: isActivityKind(value.kind) ? value.kind : 'tool',
          title: value.title,
          detail: typeof value.detail === 'string' ? value.detail : null,
          arguments: null,
          output: null,
          image_urls: [],
          failed: false,
          complete: value.complete === true,
          file_changes: [],
          display_target: null,
          display_description: null,
          reasoning: null,
        },
        clock,
      )
      break
    }
    case 'richActivity':
      if (acceptsTurnOutput(session) && asRecord(payload)) {
        upsertActivity(session, payload as ActivityItem, clock)
      }
      break
    case 'permission': {
      const value = asRecord(payload)
      if (!acceptsTurnOutput(session) || !value || typeof value.requestId !== 'string') break
      result.permission = {
        requestId: value.requestId,
        title: typeof value.title === 'string' ? value.title : 'Permission required',
        detail: typeof value.detail === 'string' ? value.detail : '',
        titleI18n: asWireTranslation(value.titleI18n),
        detailI18n: asWireTranslation(value.detailI18n),
        options: Array.isArray(value.options)
          ? value.options.map(asPermissionOption).filter((o) => o !== null)
          : [],
      }
      session.status = 'waiting'
      break
    }
    case 'userInputRequested': {
      const value = asRecord(payload)
      if (!acceptsTurnOutput(session) || !value || typeof value.requestId !== 'string') break
      const questions = Array.isArray(value.questions)
        ? value.questions.map(asUserInputQuestion).filter((question) => question !== null)
        : []
      if (!questions.length) break
      result.userInput = { requestId: value.requestId, questions }
      session.status = 'waiting'
      break
    }
    case 'usageUpdated': {
      const value = asRecord(payload)
      if (!value) break
      const previous = session.context_usage ?? { tokens: 0, window: null }
      session.context_usage = {
        tokens:
          typeof value.contextTokens === 'number' ? value.contextTokens : previous.tokens,
        window:
          typeof value.contextWindow === 'number'
            ? value.contextWindow
            : previous.window,
      }
      break
    }
    case 'goalUpdated': {
      // Conversation meta like usage: it applies regardless of turn state,
      // and `null` means the provider cleared the goal.
      const goal = asThreadGoal(payload)
      if (goal && session.messages.length === 0) {
        // A goal-first task is named after its objective until the provider
        // reports a better title.
        setTitleFromPrompt(session, goal.objective)
      }
      session.thread_goal = goal
      break
    }
    case 'turnFinished': {
      const value = asRecord(payload)
      const success = value?.success === true
      result.settled = settleTurn(
        session,
        success ? 'completed' : 'failed',
        typeof value?.summary === 'string' ? value.summary : null,
        clock,
      )
      result.permission = null
      result.userInput = null
      break
    }
    case 'localizedError': {
      const value = asRecord(payload)
      if (!value || typeof value.message !== 'string') break
      result.error = value.message
      result.errorI18n = asWireTranslation(value.i18n)
      failTurn(session, value.message, clock)
      break
    }
    case 'error': {
      if (typeof payload !== 'string') break
      result.error = payload
      failTurn(session, payload, clock)
      break
    }
    case 'processExited':
      result.settled = settleTurn(
        session,
        'failed',
        processExitError ?? 'The agent exited before responding.',
        clock,
      )
      result.permission = null
      result.userInput = null
      result.removeRuntime = true
      break
    default:
      break
  }

  session.updated_at = clock.nowSeconds()
  return result
}

function asUserInputQuestion(value: unknown): PendingUserInput['questions'][number] | null {
  const question = asRecord(value)
  if (!question || typeof question.id !== 'string' || typeof question.question !== 'string') {
    return null
  }
  return {
    id: question.id,
    header: typeof question.header === 'string' ? question.header : 'Question',
    question: question.question,
    options: Array.isArray(question.options)
      ? question.options.flatMap((value) => {
          const option = asRecord(value)
          return option && typeof option.label === 'string'
            ? [{
                label: option.label,
                ...(typeof option.description === 'string'
                  ? { description: option.description }
                  : {}),
              }]
            : []
        })
      : [],
    multiSelect: question.multiSelect === true,
  }
}

/** A running turn that already has a user message is the submitter's own
 * turn, or one hydrated after the submission was saved: leave it. A running
 * turn without one is a provider-started turn this client was following, and
 * the submission is its prompt. Otherwise open the turn here as the submitter
 * did, under the submitter's ids. */
function adoptSubmittedPrompt(
  session: AgentSession,
  message: string,
  turnId: string,
  messageId: string,
  sentByTask: string | null,
  hidden: boolean,
  clock: ReducerClock,
) {
  const now = clock.nowSeconds()
  // The daemon reuses a mirrored queue entry's id as the delivered
  // message's id, so a parked agent chip converts into this turn's prompt
  // even if its queue-change event was missed.
  session.queued_messages = (session.queued_messages ?? []).filter(
    (queued) => queued.id !== messageId,
  )
  const active = activeTurn(session)
  if (active) {
    const hasPrompt = session.messages.some(
      (candidate) => candidate.turn_id === active.id && candidate.role === 'user',
    )
    if (hasPrompt) return
    session.messages.push({
      id: messageId,
      turn_id: active.id,
      role: 'user',
      content: message,
      sent_by_task: sentByTask,
      hidden,
      created_at: now,
      streaming: false,
    })
    return
  }
  // A hidden prompt is provider-facing text, not a user draft — the title
  // keeps the words a human actually typed.
  if (!hidden) setTitleFromPrompt(session, message)
  session.turns.push({
    id: turnId,
    turn_count: session.turns.length + 1,
    status: 'running',
    provider_turn_started: false,
    provider_resume_at: null,
    started_at: now,
    completed_at: null,
    checkpoint: null,
  })
  session.messages.push({
    id: messageId,
    turn_id: turnId,
    role: 'user',
    content: message,
    sent_by_task: sentByTask,
    hidden,
    created_at: now,
    streaming: false,
  })
  session.status = 'connecting'
  session.last_reply_at = now
}

function appendText(session: AgentSession, delta: string, clock: ReducerClock) {
  if (!delta) return
  completeReasoning(session)
  const previous = session.messages.at(-1)
  if (previous?.role === 'assistant' && previous.streaming) {
    previous.content += delta
  } else {
    session.messages.push({
      id: clock.randomUUID(),
      turn_id: activeTurn(session)?.id ?? null,
      role: 'assistant',
      content: delta,
      created_at: clock.nowSeconds(),
      streaming: true,
    })
  }
}

function appendReasoning(session: AgentSession, delta: string, clock: ReducerClock) {
  if (!delta.trim() && !lastReasoning(session)) return
  finishStreamingMessages(session)
  const existing = lastReasoning(session)
  if (existing && !existing.activity.complete) {
    existing.activity.reasoning!.content += delta
    existing.activity.reasoning!.finished_at_ms = clock.nowMillis()
    return
  }
  const now = clock.nowMillis()
  pushActivity(session, {
    id: clock.randomUUID(),
    source_id: null,
    kind: 'reasoning',
    title: 'Reasoning',
    detail: null,
    arguments: null,
    output: null,
    image_urls: [],
    failed: false,
    complete: false,
    file_changes: [],
    display_target: null,
    display_description: null,
    reasoning: { content: delta, started_at_ms: now, finished_at_ms: now },
  })
}

function upsertActivity(
  session: AgentSession,
  incoming: ActivityItem,
  _clock: ReducerClock,
) {
  finishStreamingMessages(session)
  completeReasoning(session)
  for (const block of [...session.transcript_blocks].reverse()) {
    const activities = ensureActivities(block)
    const matching = [...activities].reverse().find((activity) =>
      incoming.source_id
        ? activity.source_id === incoming.source_id
        : activity.title === incoming.title && !activity.complete,
    )
    if (!matching) continue
    Object.assign(matching, {
      ...incoming,
      id: matching.id,
      tool_name: incoming.tool_name ?? matching.tool_name,
      mcp_server: incoming.mcp_server ?? matching.mcp_server,
      detail: incoming.detail ?? matching.detail,
      arguments: incoming.arguments ?? matching.arguments,
      output: incoming.output ?? matching.output,
      image_urls: incoming.image_urls?.length ? incoming.image_urls : matching.image_urls,
      file_changes: incoming.file_changes?.length
        ? incoming.file_changes
        : matching.file_changes,
      display_target: incoming.display_target ?? matching.display_target,
      display_description: incoming.display_description ?? matching.display_description,
      reasoning: incoming.reasoning ?? matching.reasoning,
    })
    return
  }
  pushActivity(session, incoming)
}

function pushActivity(session: AgentSession, activity: ActivityItem) {
  const afterMessage = session.messages.length
  const turnId = activeTurn(session)?.id ?? null
  const last = session.transcript_blocks.at(-1)
  if (last && last.after_message === afterMessage && last.turn_id === turnId) {
    ensureActivities(last).push(activity)
    return
  }
  session.transcript_blocks.push({
    after_message: afterMessage,
    turn_id: turnId,
    content: { kind: 'activities', data: [activity] },
  })
}

function settleTurn(
  session: AgentSession,
  status: TurnStatus,
  fallback: string | null,
  clock: ReducerClock,
): boolean {
  finishStreamingMessages(session)
  completeActivities(session)
  const turn = activeTurn(session)
  if (!turn) return false
  const hasAssistant = session.messages.some(
    (message) => message.turn_id === turn.id && message.role === 'assistant',
  )
  if (!hasAssistant) {
    session.messages.push({
      id: clock.randomUUID(),
      turn_id: turn.id,
      role: 'assistant',
      content:
        fallback ??
        (status === 'completed'
          ? 'The turn completed without a text response.'
          : 'The turn stopped before a response.'),
      created_at: clock.nowSeconds(),
      streaming: false,
    })
  }
  turn.status = status
  turn.completed_at = clock.nowSeconds()
  session.last_reply_at = turn.completed_at
  session.status = status === 'completed' ? 'idle' : 'failed'
  return true
}

function finishStreamingMessages(session: AgentSession) {
  for (const message of session.messages) {
    if (message.role === 'assistant') message.streaming = false
  }
}

function completeReasoning(session: AgentSession) {
  const reasoning = lastReasoning(session)
  if (reasoning) reasoning.activity.complete = true
}

function completeActivities(session: AgentSession) {
  for (const block of session.transcript_blocks) {
    for (const activity of ensureActivities(block)) activity.complete = true
  }
}

function lastReasoning(session: AgentSession) {
  const block = session.transcript_blocks.at(-1)
  const activity = block ? ensureActivities(block).at(-1) : undefined
  return activity?.reasoning ? { activity } : null
}

export function activitiesForBlock(block: TranscriptBlock): ActivityItem[] {
  if (block.content.kind === 'activities') return block.content.data
  const reasoning = block.content.data
  return [
    {
      id: `legacy-reasoning-${block.after_message}`,
      source_id: null,
      kind: 'reasoning',
      title: 'Reasoning',
      detail: null,
      arguments: null,
      output: null,
      image_urls: [],
      failed: false,
      complete: true,
      file_changes: [],
      display_target: null,
      display_description: null,
      reasoning,
    },
  ]
}

function ensureActivities(block: TranscriptBlock): ActivityItem[] {
  if (block.content.kind === 'activities') return block.content.data
  const activities = activitiesForBlock(block)
  block.content = { kind: 'activities', data: activities }
  return activities
}

function asThreadGoal(payload: unknown): ThreadGoal | null {
  const value = asRecord(payload)
  if (!value || typeof value.objective !== 'string' || typeof value.status !== 'string') {
    return null
  }
  return value as unknown as ThreadGoal
}

/** Mirror of the desktop's `strip_injected_prompt_blocks`: the daemon prepends
 * `<project-map>`/`<project-memory>` context to a session's first prompt, and a
 * provider can report that text back as a title. An opener whose closer was
 * truncated away is removed only when nothing precedes it — a mid-title
 * mention is real text. */
function stripInjectedPromptBlocks(text: string) {
  let cleaned = text
  let before = -1
  while (cleaned.length !== before) {
    before = cleaned.length
    for (const tag of ['project-map', 'project-memory']) {
      const open = `<${tag}>`
      const close = `</${tag}>`
      for (;;) {
        const start = cleaned.indexOf(open)
        if (start === -1) break
        const end = cleaned.indexOf(close, start)
        if (end === -1) {
          if (!cleaned.slice(0, start).trim()) cleaned = cleaned.slice(0, start)
          break
        }
        cleaned = cleaned.slice(0, start) + cleaned.slice(end + close.length)
      }
    }
  }
  return cleaned.trim()
}

/** Mirror of the desktop's prompt-derived title fallback: first seven words,
 * ellipsized at 54 characters, applied only while the task is unnamed. */
function setTitleFromPrompt(session: AgentSession, prompt: string) {
  if (session.messages.length > 0 || session.title !== 'New task' || session.auto_title) return
  let title = stripInjectedPromptBlocks(prompt).split(/\s+/u).filter(Boolean).slice(0, 7).join(' ')
  if (!title) return
  if ([...title].length > 54) title = `${[...title].slice(0, 53).join('')}…`
  session.auto_title = title
}

function activeTurn(session: AgentSession) {
  const turn = session.turns.at(-1)
  return turn?.status === 'running' ? turn : undefined
}

function acceptsTurnOutput(session: AgentSession) {
  return Boolean(
    activeTurn(session)
      && ['connecting', 'working', 'waiting', 'background'].includes(session.status),
  )
}

function isActivityKind(value: unknown): value is ActivityKind {
  return (
    typeof value === 'string' &&
    [
      'reasoning',
      'command',
      'fileChange',
      'fileRead',
      'fileSearch',
      'fileList',
      'search',
      'plan',
      'tool',
    ].includes(value)
  )
}

function asPermissionOption(
  value: unknown,
): { id: string; label: string; labelI18n?: WireTranslation; allow: boolean } | null {
  const option = asRecord(value)
  if (!option
    || typeof option.id !== 'string'
    || typeof option.label !== 'string'
    || typeof option.allow !== 'boolean') {
    return null
  }
  return {
    id: option.id,
    label: option.label,
    labelI18n: asWireTranslation(option.labelI18n),
    allow: option.allow,
  }
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null
}

function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T
}
