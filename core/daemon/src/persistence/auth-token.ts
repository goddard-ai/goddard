import { mkdir, readFile, writeFile } from "node:fs/promises"
import { dirname } from "node:path"
import { getAuthTokenPath } from "@goddard-ai/paths"

/** JSON shape persisted for daemon-owned backend auth state. */
type ConfigFile = {
  token?: string
}

/** Private daemon store for the backend auth token. */
export class DaemonAuthTokenStore {
  readonly #path: string

  constructor(path = getAuthTokenPath()) {
    this.#path = path
  }

  async getToken(): Promise<string | null> {
    const config = await this.#readConfig()
    return config.token ?? null
  }

  async setToken(token: string): Promise<void> {
    await this.#writeConfig({ token })
  }

  async clearToken(): Promise<void> {
    await this.#writeConfig({})
  }

  async #readConfig(): Promise<ConfigFile> {
    try {
      const raw = await readFile(this.#path, "utf-8")
      return JSON.parse(raw) as ConfigFile
    } catch {
      return {}
    }
  }

  async #writeConfig(config: ConfigFile): Promise<void> {
    await mkdir(dirname(this.#path), { recursive: true })
    await writeFile(this.#path, `${JSON.stringify(config, null, 2)}\n`, "utf-8")
  }
}
