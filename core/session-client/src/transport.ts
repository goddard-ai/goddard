export type ServerEndpoint =
  | { kind: "tcp"; port: number; url: string }
  | { kind: "ipc"; socketPath: string; url: string }

// The client consumes the server's advertised URL verbatim so it never guesses
// a default location and accidentally connects to the wrong session.
export function createClientUrl(endpoint?: ServerEndpoint): string {
  if (!endpoint) {
    throw new Error("A server endpoint is required")
  }

  return endpoint.url
}
