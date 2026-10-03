import type { AgentSession, EvalAnswer, GoalOperation, ThreadGoal, ThreadGoalStatus } from './generated'

export const MANAGED_GOAL_QUESTION = {
  next: {
    type: 'choice' as const,
    instructions: 'Given the goal and the conversation since it was set, what should happen next? Judge actual progress, not just the assistant’s claim. If the user must answer a question or choose an option, select needs_input. If an external obstacle prevents progress, select blocked. Select continue only when the agent can make a useful next attempt on its own.',
    criteria: {
      complete: 'The goal has been achieved and the result is adequately verified.',
      continue: 'Useful work remains and the agent can proceed without user input.',
      needs_input: 'A user decision or missing information is required.',
      blocked: 'An external obstacle prevents further useful work.',
    },
  },
}

export const MANAGED_GOAL_PROMPT_PREFIX = 'Goddard goal: '

export function managedGoalPrompt(objective: string, continuing: boolean): string {
  const instruction = continuing
    ? 'Build on the work already done in this task.'
    : 'Start working toward this goal.'
  return `${MANAGED_GOAL_PROMPT_PREFIX}${objective}\n${instruction} If you need a decision or information from the user, ask and stop.`
}

export function managedGoalOperation(
  session: AgentSession,
  operation: GoalOperation,
  randomUUID: () => string = () => crypto.randomUUID(),
): { session: AgentSession; prompt: string | null } {
  if (operation.kind === 'refresh') return { session, prompt: null }
  const previousObjective = session.thread_goal?.managedId ? session.thread_goal.objective : null
  const base = {
    ...session,
    queued_messages: session.queued_messages?.filter(
      (message) => previousObjective === null
        || (message.content !== managedGoalPrompt(previousObjective, false)
          && message.content !== managedGoalPrompt(previousObjective, true)),
    ),
  }
  if (operation.kind === 'clear') return { session: { ...base, thread_goal: null }, prompt: null }
  const current = base.thread_goal
  const fresh = operation.objective !== null && (operation.replace || !current?.managedId)
  let goal: ThreadGoal
  let prompt: string | null = null
  if (fresh) {
    goal = {
      objective: operation.objective!,
      status: operation.status ?? 'active',
      managedSinceMessage: base.messages.length,
      managedId: randomUUID(),
      managedLastTurn: null,
      tokenBudget: null,
      tokensUsed: 0,
      timeUsedSeconds: 0,
    }
    if (goal.status === 'active') prompt = managedGoalPrompt(goal.objective, false)
  } else if (current) {
    goal = { ...current }
    if (operation.objective !== null) {
      goal.objective = operation.objective
      goal.managedId = randomUUID()
      goal.managedSinceMessage = base.messages.length
      goal.managedLastTurn = null
      if (goal.status === 'active') prompt = managedGoalPrompt(goal.objective, true)
    }
    if (operation.status !== null) {
      if (goal.status !== operation.status) goal.managedId = randomUUID()
      goal.status = operation.status
      if (goal.status === 'active') prompt = managedGoalPrompt(goal.objective, true)
    }
  } else {
    return { session: base, prompt: null }
  }
  return { session: { ...base, thread_goal: goal }, prompt }
}

export function continuationPrompt(objective: string): string {
  return managedGoalPrompt(objective, true)
}

export function managedGoalEvaluation(session: AgentSession): {
  goalId: string
  turnId: string
  state: unknown
  stop: boolean
} | null {
  const goal = session.thread_goal
  const turn = session.turns.at(-1)
  const since = goal?.managedSinceMessage
  if (!goal?.managedId || goal.status !== 'active' || since == null || !turn
    || turn.status === 'running' || goal.managedLastTurn === turn.id) return null
  const messages = session.messages.slice(since)
  if (!messages.some((message) => message.turn_id === turn.id)) return null
  const firstTurnId = messages.find((message) => message.turn_id)?.turn_id
  const firstTurnIndex = session.turns.findIndex((item) => item.id === firstTurnId)
  const conversation = messages
    .filter((message) => message.role === 'user' || message.role === 'assistant')
    .slice(-40)
    .map((message) => ({ role: message.role, text: message.content.slice(0, 4000) }))
  const goalTurnIds = new Set(messages.map((message) => message.turn_id).filter(Boolean))
  const activities = session.transcript_blocks
    .filter((block) => block.turn_id && goalTurnIds.has(block.turn_id)
      && block.content.kind === 'activities')
    .flatMap((block) => block.content.kind === 'activities' ? block.content.data : [])
    .slice(-60)
    .map((activity) => ({
      title: activity.title,
      tool: activity.tool_name ?? null,
      failed: activity.failed,
      detail: activity.detail?.slice(0, 250) ?? '',
      output: activity.output?.slice(-500) ?? '',
    }))
  return {
    goalId: goal.managedId,
    turnId: turn.id,
    state: { goal: goal.objective, conversation, activities },
    stop: turn.status !== 'completed' || firstTurnIndex < 0
      || session.turns.length - firstTurnIndex > 20,
  }
}

export function managedGoalDecision(answer: EvalAnswer | undefined): ThreadGoalStatus | 'continue' {
  if (answer?.type !== 'choice') return 'paused'
  const confidence = answer.confidence ?? 0
  if (answer.choice === 'complete' && confidence >= 0.7) return 'complete'
  if (answer.choice === 'continue' && confidence >= 0.65) return 'continue'
  if (answer.choice === 'blocked' && confidence >= 0.5) return 'blocked'
  return 'paused'
}
