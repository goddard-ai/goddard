import type { SessionEndpoint } from "@goddard-ai/session-protocol"

// The client consumes the server's advertised URL verbatim so it never guesses
// a default location and accidentally connects to the wrong session.
export function createClientUrl(endpoint?: SessionEndpoint): string {
  if (!endpoint) {
    throw new Error("A server endpoint is required")
  }

  return endpoint.url
}
