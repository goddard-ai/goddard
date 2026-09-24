import { describe, expect, test } from 'bun:test'

import { reduceRuntimeEvent } from './event-reducer'
import { activityDisclosureSections, activityDisplayTitle, wireTranslationText } from './transcript-presentation'
import type { AgentSession, SequencedEvent } from './generated'

const clock = {
  nowSeconds: () => 200,
  nowMillis: () => 200_000,
  randomUUID: (() => {
    let id = 0
    return () => `00000000-0000-4000-8000-${String(++id).padStart(12, '0')}`
  })(),
}

const SUBMISSION = {
  message: 'Second prompt',
  turnId: '10000000-0000-4000-8000-000000000002',
  messageId: '20000000-0000-4000-8000-000000000002',
}

describe('promptSubmitted', () => {
  test('a client following the runtime mirrors another client’s submission under its ids', () => {
    // The desktop stayed attached to the idle runtime after the first turn;
    // the phone then submitted the second prompt.
    const result = reduceRuntimeEvent(idleSession(), event('promptSubmitted', SUBMISSION), clock)
    const session = result.session

    expect(session.status).toBe('connecting')
    expect(session.turns).toHaveLength(2)
    expect(session.turns.at(-1)).toMatchObject({
      id: SUBMISSION.turnId,
      turn_count: 2,
      status: 'running',
      provider_turn_started: false,
    })
    expect(session.messages.at(-1)).toMatchObject({
      id: SUBMISSION.messageId,
      turn_id: SUBMISSION.turnId,
      role: 'user',
      content: 'Second prompt',
    })

    // The provider's start confirms that turn instead of inventing one, so
    // the reply streams under the prompt that asked for it.
    const started = apply(session, 'turnStarted', null)
    expect(started.turns).toHaveLength(2)
    expect(started.turns.at(-1)?.provider_turn_started).toBe(true)
    const replied = apply(started, 'textDelta', 'Sure.')
    expect(replied.messages.map((message) => message.role)).toEqual([
      'user',
      'assistant',
      'user',
      'assistant',
    ])
    expect(replied.messages.at(-1)?.turn_id).toBe(SUBMISSION.turnId)
  })

  test('the submitting client’s own echo changes nothing', () => {
    const session = runningSession()
    const result = reduceRuntimeEvent(
      session,
      event('promptSubmitted', {
        message: 'Go',
        turnId: SUBMISSION.turnId,
        messageId: SUBMISSION.messageId,
      }),
      clock,
    )

    expect(result.session.turns).toEqual(session.turns)
    expect(result.session.messages).toEqual(session.messages)
    expect(result.session.status).toBe('connecting')
  })

  test('a provider-started turn without a prompt receives the submitted message', () => {
    const session: AgentSession = {
      ...idleSession(),
      status: 'working',
      turns: [
        ...idleSession().turns,
        {
          id: 'provider-turn',
          turn_count: 2,
          status: 'running',
          provider_turn_started: true,
          provider_resume_at: null,
          started_at: 150,
          completed_at: null,
          checkpoint: null,
        },
      ],
    }
    const result = reduceRuntimeEvent(session, event('promptSubmitted', SUBMISSION), clock)

    expect(result.session.turns).toHaveLength(2)
    expect(result.session.messages.at(-1)).toMatchObject({
      id: SUBMISSION.messageId,
      turn_id: 'provider-turn',
      role: 'user',
      content: 'Second prompt',
    })
  })

  test('names an unnamed task after its first submitted prompt', () => {
    const session: AgentSession = { ...idleSession(), messages: [], turns: [] }
    const result = reduceRuntimeEvent(
      session,
      event('promptSubmitted', { ...SUBMISSION, message: 'Add a dark mode toggle to settings' }),
      clock,
    )

    expect(result.session.auto_title).toBe('Add a dark mode toggle to settings')
    expect(result.session.turns.at(-1)?.turn_count).toBe(1)
  })

  test('injected context blocks never reach the prompt-derived title', () => {
    const session: AgentSession = { ...idleSession(), messages: [], turns: [] }
    const injected =
      '<project-map>\nA structural map.\n</project-map>\n\n' +
      '<project-memory>\nDistilled notes.\n</project-memory>\n\n' +
      'strip the blocks from session titles'
    const result = reduceRuntimeEvent(
      session,
      event('promptSubmitted', { ...SUBMISSION, message: injected }),
      clock,
    )

    expect(result.session.auto_title).toBe('strip the blocks from session titles')
  })

  test('a provider title carrying injected context is stripped or dropped', () => {
    const session = idleSession()
    const echoed = reduceRuntimeEvent(
      session,
      event(
        'autoTitleUpdated',
        '<project-memory>\nnotes\n</project-memory>\n\nFix the title lookup',
      ),
      clock,
    )
    expect(echoed.session.auto_title).toBe('Fix the title lookup')

    // A stored placeholder truncated inside the block leaves nothing usable.
    const truncated = reduceRuntimeEvent(
      session,
      event('autoTitleUpdated', '<project-memory>\nThis project has persistent me'),
      clock,
    )
    expect(truncated.session.auto_title).toBeNull()
  })

  test('a delivered agent prompt drops its chip and carries provenance', () => {
    const queuedId = '30000000-0000-4000-8000-000000000003'
    const session: AgentSession = {
      ...idleSession(),
      queued_messages: [
        {
          id: queuedId,
          content: 'Agent follow-up',
          source: { agent: { sentBy: 'sender-task' } },
          created_at: 120,
        },
        { id: 'user-queued', content: 'Mine next', created_at: 130 },
      ],
    }
    const result = reduceRuntimeEvent(
      session,
      event('promptSubmitted', {
        message: 'Agent follow-up',
        turnId: SUBMISSION.turnId,
        messageId: queuedId,
        sentByTask: 'sender-task',
      }),
      clock,
    )

    // The mirrored entry became the turn's prompt; the client-owned draft
    // behind it is untouched.
    expect(result.session.queued_messages).toMatchObject([
      { id: 'user-queued' },
    ])
    expect(result.session.messages.at(-1)).toMatchObject({
      id: queuedId,
      role: 'user',
      sent_by_task: 'sender-task',
    })
  })
})

describe('queuedMessagesChanged', () => {
  test('the daemon’s agent slice replaces itself without touching user entries', () => {
    const session: AgentSession = {
      ...idleSession(),
      queued_messages: [
        { id: 'stale-agent', content: 'Delivered already', source: { agent: { sentBy: null } }, created_at: 50 },
        { id: 'user-queued', content: 'Mine', created_at: 60 },
      ],
    }
    const result = reduceRuntimeEvent(
      session,
      event('queuedMessagesChanged', {
        messages: [
          { id: 'new-agent', content: 'Parked prompt', source: { agent: { sentBy: 'sender' } }, created_at: 70 },
        ],
      }),
      clock,
    )

    expect(result.session.queued_messages).toMatchObject([
      { id: 'user-queued' },
      { id: 'new-agent', source: { agent: { sentBy: 'sender' } } },
    ])
  })

  test('a cancellation snapshot empties the agent slice', () => {
    const session: AgentSession = {
      ...idleSession(),
      queued_messages: [
        { id: 'user-queued', content: 'Mine', created_at: 60 },
        { id: 'agent', content: 'Parked', source: { agent: { sentBy: null } }, created_at: 70 },
      ],
    }
    const result = reduceRuntimeEvent(
      session,
      event('queuedMessagesChanged', { messages: [] }),
      clock,
    )

    expect(result.session.queued_messages).toMatchObject([{ id: 'user-queued' }])
  })
})

test('MCP tool identity survives partial updates and stays separate in expanded details', () => {
  const call = {
    id: 'tool-1', source_id: 'call-1', kind: 'tool' as const,
    title: 'List running apps via CUA', detail: null,
    tool_name: 'js', mcp_server: 'goddard_js_repl', arguments: '{}',
    failed: false, complete: false,
  }
  const started = apply(runningSession(), 'richActivity', call)
  const completed = apply(started, 'richActivity', {
    ...call, id: 'update-1', tool_name: null, mcp_server: null, arguments: null,
    output: '2 apps', complete: true,
  })
  const content = completed.transcript_blocks[0]!.content
  if (content.kind !== 'activities') throw new Error('Expected an activity block')
  const item = content.data[0]!
  expect(item.id).toBe('tool-1')
  expect(item.title).toBe('List running apps via CUA')
  expect(activityDisclosureSections(item)).toEqual([
    { kind: 'mcp-server', label: 'MCP server', content: 'goddard_js_repl' },
    { kind: 'tool-name', label: 'Tool', content: 'js' },
    { kind: 'arguments', label: 'Arguments', content: '{}' },
    { kind: 'output', label: 'Output', content: '2 apps' },
  ])
  expect(activityDisclosureSections({
    ...call, mcp_server: null, tool_name: 'read_file', arguments: null, detail: 'Permission denied',
  })).toEqual([
    { kind: 'tool-name', label: 'Tool', content: 'read_file' },
    { kind: 'detail', label: null, content: 'Permission denied' },
  ])
})

function apply(session: AgentSession, kind: string, payload: unknown) {
  return reduceRuntimeEvent(session, event(kind, payload), clock).session
}

function event(kind: string, payload: unknown): SequencedEvent {
  return {
    sessionId: 'session',
    runtimeId: 'runtime',
    epoch: 'epoch',
    sequence: 1,
    event: { kind, payload: payload as never },
  }
}

/** One completed turn, runtime still attached, nothing running. */
function idleSession(): AgentSession {
  return {
    ...runningSession(),
    status: 'idle',
    messages: [
      {
        id: 'message',
        turn_id: 'turn',
        role: 'user',
        content: 'Go',
        created_at: 100,
        streaming: false,
      },
      {
        id: 'reply',
        turn_id: 'turn',
        role: 'assistant',
        content: 'Done',
        created_at: 110,
        streaming: false,
      },
    ],
    turns: [
      {
        id: 'turn',
        turn_count: 1,
        status: 'completed',
        provider_turn_started: true,
        provider_resume_at: null,
        started_at: 100,
        completed_at: 110,
        checkpoint: null,
      },
    ],
  }
}

function runningSession(): AgentSession {
  return {
    id: 'session',
    title: 'New task',
    project_id: 'project',
    workspace: { kind: 'local' },
    provider: 'codex',
    runtime_mode: 'fullAccess',
    status: 'connecting',
    created_at: 100,
    updated_at: 100,
    provider_cursor: null,
    messages: [
      {
        id: 'message',
        turn_id: 'turn',
        role: 'user',
        content: 'Go',
        created_at: 100,
        streaming: false,
      },
    ],
    transcript_blocks: [],
    turns: [
      {
        id: 'turn',
        turn_count: 1,
        status: 'running',
        provider_turn_started: false,
        provider_resume_at: null,
        started_at: 100,
        completed_at: null,
        checkpoint: null,
      },
    ],
  }
}

describe('localized events', () => {
  test('localizedError carries its i18n semantic beside the fallback message', () => {
    const session = runningSession()
    const result = reduceRuntimeEvent(
      session,
      event('localizedError', {
        message: 'Claude has no active turn',
        i18n: { key: 'errors.provider_no_active_turn', args: { provider: 'Claude' } },
      }),
      clock,
    )
    expect(result.error).toBe('Claude has no active turn')
    expect(result.errorI18n).toMatchObject({
      key: 'errors.provider_no_active_turn',
      args: { provider: 'Claude' },
    })
    // The turn failure behaves exactly like an opaque provider error.
    expect(result.session.status).toBe('failed')
    expect(result.session.messages.at(-1)).toMatchObject({
      role: 'assistant',
      content: 'Claude has no active turn',
    })
  })

  test('permission keeps title/detail fallbacks and their i18n semantics', () => {
    const result = reduceRuntimeEvent(
      runningSession(),
      event('permission', {
        requestId: 'per_1',
        title: 'npm test',
        titleI18n: { key: 'permission.run_tool', args: { tool: 'npm' } },
        detail: 'The agent wants to run npm',
        detailI18n: { key: 'permission.agent_wants_to_run', args: { tool: 'npm' } },
        options: [{ id: 'allow', label: 'Allow once', allow: true }],
      }),
      clock,
    )
    expect(result.permission).toMatchObject({
      title: 'npm test',
      detail: 'The agent wants to run npm',
      titleI18n: { key: 'permission.run_tool' },
      detailI18n: { key: 'permission.agent_wants_to_run' },
    })
  })

  test('permission without i18n fields decodes with undefined semantics', () => {
    const result = reduceRuntimeEvent(
      runningSession(),
      event('permission', {
        requestId: 'per_1',
        title: 'rm -rf *',
        detail: 'provider text',
        options: [],
      }),
      clock,
    )
    expect(result.permission?.titleI18n).toBeUndefined()
    expect(result.permission?.detailI18n).toBeUndefined()
  })
})

describe('agent rename requests', () => {
  const renameEvent = () =>
    event('permission', {
      requestId: 'agent-rename-1',
      title: 'Rename this task?',
      detail: 'The agent wants to rename this task.',
      options: [
        { id: 'once', label: 'Rename', allow: true },
        { id: 'deny', label: 'Deny', allow: false },
      ],
    })

  test('a rename request bypasses the turn gate and skips the composer lane', () => {
    // The request parks on the session, not the turn — it can legitimately
    // arrive after the turn that raised it already folded.
    const result = reduceRuntimeEvent(idleSession(), renameEvent(), clock)
    expect(result.renameRequest).toMatchObject({ requestId: 'agent-rename-1' })
    expect(result.permission).toBeUndefined()
    expect(result.session.status).toBe('waiting')
  })

  test('a finished turn does not settle a rename request', () => {
    const session = runningSession()
    const requested = reduceRuntimeEvent(session, renameEvent(), clock)
    expect(requested.renameRequest?.requestId).toBe('agent-rename-1')

    const finished = reduceRuntimeEvent(
      requested.session,
      event('turnFinished', { success: true, summary: null }),
      clock,
    )
    expect(finished.renameRequest).toBeUndefined()
    expect(finished.permission).toBeNull()
  })

  test('requestSettled names the resolved request', () => {
    const result = reduceRuntimeEvent(
      runningSession(),
      event('requestSettled', { requestId: 'agent-rename-1' }),
      clock,
    )
    expect(result.settledRequestId).toBe('agent-rename-1')
  })

  test('requestSettled returns a turnless waiting session to idle', () => {
    // The rename card was the only thing holding `waiting` — once the daemon
    // settles it nothing else will move the status back.
    const requested = reduceRuntimeEvent(idleSession(), renameEvent(), clock)
    expect(requested.session.status).toBe('waiting')
    const settled = reduceRuntimeEvent(
      requested.session,
      event('requestSettled', { requestId: requested.renameRequest!.requestId }),
      clock,
    )
    expect(settled.session.status).toBe('idle')
  })

  test('a dead runtime clears a rename request', () => {
    const requested = reduceRuntimeEvent(runningSession(), renameEvent(), clock)
    const exited = reduceRuntimeEvent(
      requested.session,
      event('processExited', null),
      clock,
    )
    expect(exited.renameRequest).toBeNull()
  })
})

test('wireTranslationText renders through the client translator or keeps the fallback', () => {
  const t = (key: string, params?: Record<string, string | number>) =>
    `${key}(${Object.entries(params ?? {}).map(([k, v]) => `${k}=${v}`).join(',')})`
  const i18n = { key: 'activity.search_for', args: { query: 'cats' } }
  expect(wireTranslationText(i18n, 'Searching for cats', t)).toBe('activity.search_for(query=cats)')
  expect(wireTranslationText(i18n, 'Searching for cats')).toBe('Searching for cats')
  expect(wireTranslationText(undefined, 'provider text', t)).toBe('provider text')
})

test('a keyed activity title renders in the client locale before heuristics', () => {
  const activity = {
    id: 'a1',
    kind: 'search',
    title: 'Searching for cats',
    title_i18n: { key: 'activity.search_for', args: { query: 'cats' } },
    complete: true,
    failed: false,
  } as unknown as Parameters<typeof activityDisplayTitle>[0]
  const t = (key: string, params?: Record<string, string | number>) =>
    `${key}(${params?.query ?? ''})`
  expect(activityDisplayTitle(activity, t)).toBe('activity.search_for(cats)')
  expect(activityDisplayTitle(activity)).toBe('Searching for cats')
})
