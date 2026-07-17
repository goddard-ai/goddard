import type { DaemonSession, SetSessionConfigOptionRequest } from "@goddard-ai/sdk"

/** Applies session config changes in order and refreshes live state after a partial failure. */
export async function applySessionControlUpdates(input: {
  updates: SetSessionConfigOptionRequest[]
  apply: (update: SetSessionConfigOptionRequest) => Promise<{ session: DaemonSession }>
  refresh: () => Promise<DaemonSession>
  sync: (session: DaemonSession) => void
}) {
  try {
    for (const update of input.updates) {
      const result = await input.apply(update)
      input.sync(result.session)
    }
  } catch (error) {
    try {
      input.sync(await input.refresh())
    } catch {
      // Keep the original client-safe configuration error as the actionable failure.
    }

    throw error
  }
}
