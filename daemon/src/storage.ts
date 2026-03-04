import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { homedir } from "node:os";
import type { TokenStorage } from "@goddard-ai/sdk";

type CredentialsFile = {
  token?: string;
};

export class FileTokenStorage implements TokenStorage {
  readonly #path: string;

  constructor(path = join(homedir(), ".goddard", "credentials.json")) {
    this.#path = path;
  }

  async getToken(): Promise<string | null> {
    const config = await this.#readConfig();
    return config.token ?? null;
  }

  async setToken(token: string): Promise<void> {
    await this.#writeConfig({ token });
  }

  async clearToken(): Promise<void> {
    await this.#writeConfig({});
  }

  async #readConfig(): Promise<CredentialsFile> {
    try {
      const raw = await readFile(this.#path, "utf-8");
      return JSON.parse(raw) as CredentialsFile;
    } catch {
      return {};
    }
  }

  async #writeConfig(config: CredentialsFile): Promise<void> {
    await mkdir(dirname(this.#path), { recursive: true });
    await writeFile(this.#path, `${JSON.stringify(config, null, 2)}\n`, "utf-8");
  }
}
