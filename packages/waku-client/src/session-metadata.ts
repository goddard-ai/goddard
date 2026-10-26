import { WakuConnectionError, type WakuClient } from './client'
import type { SessionMetadataChange } from './generated'

/** A retry uses the same identity because the response may be lost after commit. */
export async function updateSessionMetadata(
  client: WakuClient,
  sessionId: string,
  expectedRevision: number,
  mutationId: string,
  change: SessionMetadataChange,
) {
  const command = { type: 'updateSessionMetadata', expectedRevision, mutationId, change } as const
  const request = async () => {
    const response = await client.request(command, sessionId)
    if (response.type !== 'sessionMetadataUpdated') {
      throw new Error(`Expected daemon response sessionMetadataUpdated, received ${response.type}`)
    }
    return response
  }
  try {
    return await request()
  } catch (error) {
    if (!(error instanceof WakuConnectionError) || !error.retryable) throw error
    return request()
  }
}
