import { useQuery, useQueryClient } from '@tanstack/react-query'
import type { AgentSession, Project } from '@waku/client'
import { useEffect, useState, type RefObject } from 'react'
import { toast } from 'sonner'
import { Dialog, DialogContent, DialogTitle } from '@/components/ui/dialog'
import { WakuIcon, type WakuIconName } from '@/components/waku-icon'
import {
  commitWorkspace,
  daemonKeys,
  generateWorkspaceCommitMessage,
  inspectWorkspaceCommit,
  loadDaemonSettings,
  probeProvider,
  pushWorkspace,
  sessionCwd,
} from '@/lib/daemon-api'
import { useDaemon } from '@/lib/daemon-context'
import { useI18n } from '@/lib/i18n'
import { usePrimaryShortcut } from '@/lib/platform'
import { cn } from '@/lib/utils'
import { providerMeta } from '@/components/waku-icon'

type CommitAction = 'commit' | 'commitAndPush' | 'push'

// The daemon pins commit-subject generation to these cheap tiers regardless
// of the session model (waku-protocol CLAUDE_COMMIT_MODEL/CODEX_COMMIT_MODEL).
const COMMIT_GENERATION_MODELS: Partial<Record<AgentSession['provider'], string>> = {
  claude: 'claude-haiku-4-5',
  codex: 'gpt-5.6-luna',
}

export function CommitDialog({
  open,
  session,
  project,
  returnFocus,
  onOpenChange,
}: {
  open: boolean
  session: AgentSession | null
  project?: Project
  returnFocus?: RefObject<HTMLElement | null>
  onOpenChange: (open: boolean) => void
}) {
  const { t } = useI18n()
  const { client, config, phase } = useDaemon()
  const queryClient = useQueryClient()
  const [message, setMessage] = useState('')
  const [includeUnstaged, setIncludeUnstaged] = useState(true)
  const [pending, setPending] = useState<CommitAction | 'generate' | null>(null)
  const [error, setError] = useState<string | null>(null)
  const commitShortcut = usePrimaryShortcut('⌘↩', 'Ctrl+Enter')
  const cwd = session && project ? sessionCwd(session, project) : undefined

  const snapshot = useQuery({
    queryKey: ['daemon', config?.address ?? 'disconnected', 'commit', cwd ?? 'none'],
    queryFn: () => inspectWorkspaceCommit(requireClient(client), cwd!),
    enabled: open && phase === 'connected' && Boolean(client && config && cwd),
  })

  useEffect(() => {
    if (!open) return
    setMessage('')
    setIncludeUnstaged(true)
    setPending(null)
    setError(null)
  }, [open, cwd])

  const additions = includeUnstaged
    ? snapshot.data?.additions ?? 0
    : snapshot.data?.staged_additions ?? 0
  const deletions = includeUnstaged
    ? snapshot.data?.deletions ?? 0
    : snapshot.data?.staged_deletions ?? 0
  const canCommit = !snapshot.isPending && Boolean(
    snapshot.data?.has_staged || (includeUnstaged && snapshot.data?.has_unstaged),
  )
  const canPush = !snapshot.isPending && Boolean(snapshot.data?.can_push)
  const hasMessage = Boolean(message.trim())
  const generationLabel = session
    ? [providerMeta(session.provider).name,
       COMMIT_GENERATION_MODELS[session.provider] ?? session.model]
      .filter(Boolean)
      .join(' · ')
    : null

  // Generate a subject into the field and stop — the user reviews or edits
  // it, then commits with a separate action. Generation never commits.
  async function generate() {
    if (!client || !config || !cwd || pending) return
    if (!session) {
      setError(t('commit.no_task'))
      return
    }
    setPending('generate')
    setError(null)
    try {
      const settings = await loadDaemonSettings(client)
      const probe = await probeProvider(client, session.provider, settings)
      if (!probe.installed || !probe.path) {
        throw new Error(t('commit.agent_unavailable'))
      }
      const commitMessage = await generateWorkspaceCommitMessage(
        client,
        cwd,
        includeUnstaged,
        {
          provider: session.provider,
          binary: probe.path,
          model: session.model ?? null,
          reasoning_effort: session.reasoning_effort ?? null,
        },
      )
      setMessage(commitMessage)
    } catch (cause) {
      setError(errorMessage(cause))
    } finally {
      setPending(null)
    }
  }

  async function run(action: CommitAction) {
    if (!client || !config || !cwd || pending) return
    const commitMessage = message.trim()
    // A blank message has nothing to commit — the confirm becomes a
    // generate request instead, and the subject lands for review.
    if (action !== 'push' && !commitMessage) {
      await generate()
      return
    }
    setPending(action)
    setError(null)
    try {
      if (action === 'push') {
        await pushWorkspace(client, cwd)
      } else {
        await commitWorkspace(client, cwd, commitMessage, includeUnstaged, action === 'commitAndPush')
      }
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ['daemon', config.address, 'workspace'] }),
        queryClient.invalidateQueries({ queryKey: ['daemon', config.address, 'workspace-tree'] }),
        queryClient.invalidateQueries({ queryKey: ['daemon', config.address, 'workspace-diff'] }),
      ])
      toast.success(t(action === 'commit'
        ? 'commit.committed'
        : action === 'commitAndPush'
          ? 'commit.committed_and_pushed'
          : 'commit.pushed'))
      onOpenChange(false)
    } catch (cause) {
      setError(errorMessage(cause))
      void snapshot.refetch()
    } finally {
      setPending(null)
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-[420px] overflow-hidden rounded-[18px] bg-[var(--raised)] p-0" finalFocus={returnFocus}>
        <DialogTitle className="sr-only">{t('environment.commit_or_push')}</DialogTitle>
        <div className="flex h-12 items-center gap-2.5 px-4 text-[14px]">
          <WakuIcon className="size-[15px]" name="gitBranch" />
          <span className="min-w-0 flex-1 truncate">{snapshot.data?.branch ?? 'HEAD'}</span>
        </div>
        <textarea
          autoFocus
          aria-label={t('commit.message')}
          className="h-28 w-full resize-none border-0 bg-transparent px-4 py-2.5 text-[14px] leading-[21px] outline-none placeholder:text-[var(--text-ghost)]"
          disabled={Boolean(pending)}
          placeholder={t('commit.message_placeholder')}
          value={message}
          onChange={(event) => setMessage(event.target.value)}
          onKeyDown={(event) => {
            if ((event.metaKey || event.ctrlKey) && event.key === 'Enter' && canCommit && !pending) {
              event.preventDefault()
              // Blank field: the confirm generates instead of committing.
              void (hasMessage ? run('commit') : generate())
            }
          }}
        />
        <div className="px-2">
          <button
            aria-checked={includeUnstaged}
            className="flex h-11 w-full items-center gap-2.5 rounded-[9px] px-3 text-[14px] outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-45"
            disabled={Boolean(pending)}
            role="checkbox"
            type="button"
            onClick={() => setIncludeUnstaged((current) => !current)}
          >
            <span className={cn(
              'grid size-[15px] place-items-center rounded border',
              includeUnstaged ? 'border-input bg-background' : 'border-[var(--text-ghost)]',
            )}>
              {includeUnstaged && <WakuIcon className="size-3" name="check" />}
            </span>
            <span className="min-w-0 flex-1 text-left">{t('commit.include_unstaged')}</span>
            <span className="flex items-center gap-1.5 text-[13.5px] font-medium">
              <span className="text-[var(--success)]">+{additions.toLocaleString()}</span>
              <span className="text-destructive">-{deletions.toLocaleString()}</span>
            </span>
          </button>
        </div>
        {(error || snapshot.error) && (
          <p className="px-5 pb-2 text-[11.5px] leading-4 text-destructive">
            {error ?? errorMessage(snapshot.error)}
          </p>
        )}
        <div className="mx-2 mt-1 border-t" />
        <div className="flex flex-col gap-0.5 p-2">
          <CommitActionRow
            enabled={canCommit}
            icon="sparkle"
            label={t(pending === 'generate' ? 'commit.generating_message' : 'commit.generate')}
            pending={pending === 'generate'}
            onClick={() => void generate()}
          />
          <CommitActionRow
            enabled={canCommit && hasMessage}
            icon="gitCommitHorizontal"
            label={t(pending === 'commit' ? 'commit.committing' : 'commit.commit')}
            pending={pending === 'commit'}
            shortcut={commitShortcut}
            onClick={() => void run('commit')}
          />
          <CommitActionRow
            enabled={canCommit && hasMessage}
            icon="cloudUpload"
            label={t(pending === 'commitAndPush' ? 'commit.committing_and_pushing' : 'commit.commit_and_push')}
            pending={pending === 'commitAndPush'}
            onClick={() => void run('commitAndPush')}
          />
          <CommitActionRow
            enabled={canPush}
            icon="cloudUpload"
            label={t(pending === 'push' ? 'commit.pushing' : 'commit.push')}
            pending={pending === 'push'}
            onClick={() => void run('push')}
          />
        </div>
        {generationLabel && (
          <p className="px-5 pb-2.5 text-[11.5px] text-[var(--text-ghost)]">
            {t('commit.generates_with', { name: generationLabel })}
          </p>
        )}
      </DialogContent>
    </Dialog>
  )
}

function CommitActionRow({
  icon,
  label,
  shortcut,
  enabled,
  pending,
  onClick,
}: {
  icon: WakuIconName
  label: string
  shortcut?: string
  enabled: boolean
  pending: boolean
  onClick: () => void
}) {
  return (
    <button
      className="flex min-h-9 w-full items-center gap-2.5 rounded-lg px-3 text-[13.5px] outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-40"
      disabled={!enabled || pending}
      type="button"
      onClick={onClick}
    >
      <WakuIcon className={cn('size-3.5 text-[var(--text-secondary)]', pending && 'motion-safe:animate-spin')} name={pending ? 'loaderCircle' : icon} />
      <span className="min-w-0 flex-1 truncate text-left">{label}</span>
      {shortcut && <span className="text-[11px] text-[var(--text-ghost)]">{shortcut}</span>}
    </button>
  )
}

function requireClient<T>(client: T | null): T {
  if (!client) throw new Error('Goddard daemon is disconnected')
  return client
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}
