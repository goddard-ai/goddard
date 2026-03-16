import { HttpError } from "./errors.ts"
import type { AuthorizedSession } from "./types.ts"

export async function requireAuthorizedSession(
  request: Request,
  getSessionByToken: (token: string) => Promise<AuthorizedSession | null>,
): Promise<AuthorizedSession> {
  const authorization = request.headers.get("authorization")
  if (!authorization) {
    throw new HttpError(401, "Authorization header is required")
  }

  const match = authorization.match(/^Bearer\s+(.+)$/i)
  if (!match) {
    throw new HttpError(401, "Authorization header must use Bearer auth")
  }

  const session = await getSessionByToken(match[1]!)
  if (!session) {
    throw new HttpError(401, "Invalid session token")
  }

  return session
}
