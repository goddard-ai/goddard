/** Minimal Durable Object namespace surface used by backend worker bindings. */
type DurableObjectNamespaceBinding = {
  idFromName: (name: string) => unknown
  get: (id: unknown) => DurableObjectStubBinding
}

/** Minimal Durable Object stub surface used for backend worker-internal requests. */
type DurableObjectStubBinding = {
  fetch: (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>
}

/** Cloudflare environment bindings required by the backend worker. */
export interface Env {
  TURSO_DB_URL: string
  TURSO_DB_AUTH_TOKEN: string
  GITHUB_APP_ID?: string
  GITHUB_APP_PRIVATE_KEY?: string
  USER_STREAM?: DurableObjectNamespaceBinding
  CLOUD_SESSION?: DurableObjectNamespaceBinding
  GODDARD_BACKEND_TEST_MODE?: string
}
