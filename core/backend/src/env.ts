/** Minimal Durable Object namespace surface used by the user stream binding. */
type UserStreamNamespace = {
  idFromName: (name: string) => unknown
  get: (id: unknown) => UserStreamStub
}

/** Minimal Durable Object stub surface used to publish and subscribe streams. */
type UserStreamStub = {
  fetch: (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>
}

/** Cloudflare environment bindings required by the backend worker. */
export interface Env {
  TURSO_DB_URL: string
  TURSO_DB_AUTH_TOKEN: string
  GITHUB_APP_ID?: string
  GITHUB_APP_PRIVATE_KEY?: string
  USER_STREAM?: UserStreamNamespace
}
