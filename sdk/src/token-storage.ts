export interface TokenStorage {
  getToken(): Promise<string | null>;
  setToken(token: string): Promise<void>;
  clearToken(): Promise<void>;
}

export class InMemoryTokenStorage implements TokenStorage {
  #token: string | null = null;

  async getToken(): Promise<string | null> {
    return this.#token;
  }

  async setToken(token: string): Promise<void> {
    this.#token = token;
  }

  async clearToken(): Promise<void> {
    this.#token = null;
  }
}
