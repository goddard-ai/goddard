export interface Env {
  TURSO_DB_URL: string
  TURSO_DB_AUTH_TOKEN: string
  GITHUB_APP_ID?: string
  GITHUB_APP_PRIVATE_KEY?: string
  USER_STREAM?: DurableObjectNamespace
}
