import { afterEach, expect, mock, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import type { AgentSession, Command, Project, ResponsePayload, WakuClient } from '@waku/client'
import { createRequire } from 'node:module'
import { beginTurn, createSession, daemonKeys } from './daemon-api'

// React Query is hoisted; use its React instance for this headless renderer.
const workspaceRequire = createRequire(new URL('../../../../package.json', import.meta.url))
mock.module('react', () => workspaceRequire('react'))
const { renderToStaticMarkup } = workspaceRequire('react-dom/server') as typeof import('react-dom/server')

let activeDaemon: { client: WakuClient; config: { address: string }; phase: 'connected' }
mock.module('./daemon-context', () => ({ useDaemon: () => activeDaemon }))
const { RuntimeProvider, useRuntime } = await import('./runtime-context')
const cleanups: Array<() => Promise<void>> = []
afterEach(async () => {
  for (const cleanup of cleanups.splice(0)) await cleanup()
})

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => { resolve = done })
  return { promise, resolve }
}

// Exercise real runtime callbacks and persistence through the query cache,
// without a browser, native app, or provider executable.
async function checkRefusal(replaced: boolean) {
  const before: AgentSession = {
    ...createSession('project', 'claude', false),
    model: 'selected-model', runtime_mode: 'ask', detail_loaded: true,
  }
  const project = { id: 'project', name: 'Project', path: '/repo', created_at: 1 } as Project
  const entered = deferred<Command>()
  const release = deferred<void>()
  const commands: Command[] = []
  const client = {
    connected: true,
    request: async (command: Command): Promise<ResponsePayload> => {
      commands.push(command)
      switch (command.type) {
        case 'attachSession':
          return { type: 'sessionRuntime', runtimeId: 'runtime', supportsSteer: false, supportsUserInputActions: false }
        case 'loadTaskState':
          return { type: 'taskState', projects: [project], sessions: [before], defaultCwd: '/repo', projectlessRoot: null }
        case 'saveTaskState':
          return { type: 'taskStateSaved', sessions: command.sessions }
        case 'workspace':
          return { type: 'workspace', result: { type: 'ack' } }
        case 'promptWithConfiguration':
          entered.resolve(command)
          await release.promise
          throw new Error('The selected configuration was refused')
        default:
          return { type: 'ack' }
      }
    },
    subscribe: () => () => {},
  } as unknown as WakuClient
  activeDaemon = { client, config: { address: 'fixture' }, phase: 'connected' }
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: Infinity } } })
  const key = daemonKeys.session('fixture', before.id)
  queryClient.setQueryData(key, before)
  let runtime!: ReturnType<typeof useRuntime>
  function CaptureRuntime() { runtime = useRuntime(); return null }
  renderToStaticMarkup(<QueryClientProvider client={queryClient}>
    <RuntimeProvider><CaptureRuntime /></RuntimeProvider>
  </QueryClientProvider>)
  cleanups.push(async () => { await runtime.closeSession(before.id); queryClient.clear() })
  await runtime.attachSession(before)
  const outcome = runtime.sendPrompt(before, 'Keep this user message').then(() => null, error => error)
  const command = await entered.promise
  expect(command).toMatchObject({ type: 'promptWithConfiguration', configuration: {
    provider: 'claude', options: { mode: 'ask', model: 'selected-model' },
  } })
  const current = queryClient.getQueryData<AgentSession>(key)!
  const selected = { ...current, model: 'latest-model' }
  const replacement = replaced ? beginTurn({ ...selected, history_generation: 1,
    messages: [], turns: [], status: 'idle' }, 'replacement intent') : selected
  queryClient.setQueryData(key, replacement)
  release.resolve()
  expect(String(await outcome)).toContain('selected configuration was refused')
  const latest = queryClient.getQueryData<AgentSession>(key)!
  if (replaced) {
    expect(latest).toEqual(replacement)
    return
  }
  expect(latest.model).toBe('latest-model')
  expect(latest.messages.some(message => message.role === 'user' && message.content === 'Keep this user message')).toBe(true)
  expect(latest.turns.at(-1)?.status).toBe('failed')
  expect(commands.filter(command => command.type === 'prompt')).toHaveLength(0)
}

test('configured web prompt refusal preserves a newer selection and the original user message', () => checkRefusal(false))
test('configured late web refusal cannot fail a replacement history turn', () => checkRefusal(true))
