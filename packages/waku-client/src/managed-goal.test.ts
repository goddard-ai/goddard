import { expect, test } from 'bun:test'
import type { AgentSession, EvalAnswer } from './generated'
import { managedGoalDecision, managedGoalEvaluation, managedGoalOperation } from './managed-goal'

function session(): AgentSession {
  return {
    provider: 'claude',
    messages: [{
      id: 'prior', turn_id: 'prior-turn', role: 'user', content: 'Unrelated earlier work',
      created_at: 1, streaming: false,
    }],
    turns: [{ id: 'prior-turn', turn_count: 1, status: 'completed',
      provider_turn_started: true, provider_resume_at: null,
      started_at: 1, completed_at: 2, checkpoint: null }],
    transcript_blocks: [{
      after_message: 1, turn_id: 'prior-turn',
      content: { kind: 'activities', data: [{
        id: 'prior-activity', title: 'Old command', failed: false,
      }] },
    }],
    thread_goal: null,
  } as unknown as AgentSession
}

test('managed goal evaluation excludes context before the goal', () => {
  const started = managedGoalOperation(session(), {
    kind: 'set', objective: 'Ship the feature', status: 'active', replace: false,
  }, () => 'goal-1')
  expect(started.prompt?.startsWith('Goddard goal: Ship the feature')).toBe(true)
  const current = {
    ...started.session,
    messages: [...started.session.messages, {
      id: 'new', turn_id: 'new-turn', role: 'assistant' as const, content: 'Implemented it',
      created_at: 3, streaming: false,
    }],
    turns: [...started.session.turns, {
      id: 'new-turn', turn_count: 2, status: 'completed' as const,
      provider_turn_started: true, provider_resume_at: null,
      started_at: 3, completed_at: 4, checkpoint: null,
    }],
  }
  const plan = managedGoalEvaluation(current)
  expect(plan?.goalId).toBe('goal-1')
  expect(JSON.stringify(plan?.state)).not.toContain('Unrelated earlier work')
  expect(JSON.stringify(plan?.state)).not.toContain('Old command')
  expect(JSON.stringify(plan?.state)).toContain('Implemented it')
})

test('user input and uncertain answers pause the managed loop', () => {
  const answer = (choice: string, confidence: number): EvalAnswer => ({
    type: 'choice', choice, confidence, probabilities: { [choice]: confidence },
  })
  expect(managedGoalDecision(answer('needs_input', 0.98))).toBe('paused')
  expect(managedGoalDecision(answer('continue', 0.4))).toBe('paused')
  expect(managedGoalDecision(answer('continue', 0.9))).toBe('continue')
  expect(managedGoalDecision(answer('complete', 0.9))).toBe('complete')
  expect(managedGoalDecision(undefined)).toBe('paused')
})

test('pausing removes a queued goal continuation', () => {
  const started = managedGoalOperation(session(), {
    kind: 'set', objective: 'Ship the feature', status: 'active', replace: false,
  }, () => 'goal-1')
  const queued = {
    ...started.session,
    queued_messages: [{ id: 'goal-prompt', content: started.prompt! },
      { id: 'human-prompt', content: 'Please also check the docs' }],
  } as AgentSession
  const paused = managedGoalOperation(queued, {
    kind: 'set', objective: null, status: 'paused', replace: false,
  })
  expect(paused.prompt).toBeNull()
  expect(paused.session.thread_goal?.status).toBe('paused')
  expect(paused.session.queued_messages?.map((message) => message.id)).toEqual(['human-prompt'])
})

test('editing the objective resets the evaluation boundary', () => {
  const started = managedGoalOperation(session(), {
    kind: 'set', objective: 'First goal', status: 'active', replace: false,
  }, () => 'goal-1')
  const withProgress = {
    ...started.session,
    messages: [...started.session.messages, {
      id: 'progress', turn_id: 'first-goal-turn', role: 'assistant' as const,
      content: 'Only relevant to the first goal', created_at: 3, streaming: false,
    }],
  }
  const edited = managedGoalOperation(withProgress, {
    kind: 'set', objective: 'Second goal', status: null, replace: false,
  }, () => 'goal-2')
  expect(edited.session.thread_goal?.managedId).toBe('goal-2')
  expect(edited.session.thread_goal?.managedSinceMessage).toBe(2)
  expect(edited.prompt).toContain('Second goal')
})
