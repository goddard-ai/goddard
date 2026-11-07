import { useCallback, useSyncExternalStore } from 'react'
import { landingStore, landingStatusText, type WakuClient } from '@waku/client'
import { useI18n } from '@/lib/i18n'

export function LandingStatus({ client, taskId, cwd }: { client: WakuClient; taskId: string; cwd: string }) {
  const store = landingStore(client)
  const subscribe = useCallback((listener: () => void) => store.subscribe(taskId, cwd, listener), [store, taskId, cwd])
  const get = useCallback(() => store.get(taskId, cwd), [store, taskId, cwd])
  const view = useSyncExternalStore(subscribe, get, get)
  const { t } = useI18n()
  if (!client.landingQueueAvailable) return null
  const text = landingStatusText(view, t)
  if (!text) return null
  const queued = view.phase === 'known' && view.job?.state === 'queued'
  return (
    <div className="mx-3 mb-2 flex items-center justify-between gap-3 rounded-lg border px-3 py-2 text-sm text-muted-foreground">
      <span role="status" aria-live="polite">{text}</span>
      {queued && <button type="button" className="shrink-0 rounded px-2 py-1 text-foreground hover:bg-muted focus-visible:outline-2 focus-visible:outline-ring" onClick={() => void store.cancel(taskId, cwd)}>{t('git_panel.land_cancel_queued')}</button>}
      {view.phase === 'unavailable' && <button type="button" className="shrink-0 rounded px-2 py-1 focus-visible:outline-2 focus-visible:outline-ring" onClick={() => store.retryStatus(taskId, cwd)}>{t('git_panel.land_refresh_status')}</button>}
    </div>
  )
}
