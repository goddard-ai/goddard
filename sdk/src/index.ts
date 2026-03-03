import type {
  AuthSession,
  CreatePrInput,
  DeviceFlowComplete,
  DeviceFlowSession,
  DeviceFlowStart,
  GitHubWebhookInput,
  PiAgentConfig,
  PullRequestRecord,
  RepoEvent,
  RepoRef,
  StreamMessage,
  ThinkingLevel
} from "./types.ts";
import { InMemoryTokenStorage, type TokenStorage } from "./token-storage.ts";

export const SDK_VERSION = "0.1.0";

type FetchLike = typeof fetch;
type WebSocketCtor = typeof WebSocket;

export type GoddardSdkOptions = {
  baseUrl: string;
  tokenStorage?: TokenStorage;
  fetchImpl?: FetchLike;
  webSocketImpl?: WebSocketCtor;
};

type StreamHandler = (event?: unknown) => void;

export class StreamSubscription {
  #socket: WebSocket;
  #listeners = new Map<string, Set<StreamHandler>>();

  constructor(socket: WebSocket) {
    this.#socket = socket;
  }

  on(eventName: string, handler: StreamHandler): this {
    const listeners = this.#listeners.get(eventName) ?? new Set<StreamHandler>();
    listeners.add(handler);
    this.#listeners.set(eventName, listeners);
    return this;
  }

  off(eventName: string, handler: StreamHandler): this {
    this.#listeners.get(eventName)?.delete(handler);
    return this;
  }

  emit(eventName: string, payload?: unknown): void {
    this.#listeners.get(eventName)?.forEach((listener) => listener(payload));
  }

  close(): void {
    this.#socket.close();
  }
}

export class GoddardSdk {
  readonly auth: {
    startDeviceFlow: (input?: DeviceFlowStart) => Promise<DeviceFlowSession>;
    completeDeviceFlow: (input: DeviceFlowComplete) => Promise<AuthSession>;
    whoami: () => Promise<AuthSession>;
    logout: () => Promise<void>;
  };

  readonly pr: {
    create: (input: CreatePrInput) => Promise<PullRequestRecord>;
  };

  readonly stream: {
    subscribeToRepo: (repo: RepoRef) => Promise<StreamSubscription>;
  };

  readonly #baseUrl: URL;
  readonly #tokenStorage: TokenStorage;
  readonly #fetchImpl: FetchLike;
  readonly #webSocketImpl: WebSocketCtor;

  constructor(options: GoddardSdkOptions) {
    this.#baseUrl = new URL(options.baseUrl);
    this.#tokenStorage = options.tokenStorage ?? new InMemoryTokenStorage();
    this.#fetchImpl = options.fetchImpl ?? fetch;
    this.#webSocketImpl = options.webSocketImpl ?? WebSocket;

    this.auth = {
      startDeviceFlow: async (input = {}) => {
        return this.#request<DeviceFlowSession>("POST", "/auth/device/start", input, false);
      },
      completeDeviceFlow: async (input) => {
        const session = await this.#request<AuthSession>("POST", "/auth/device/complete", input, false);
        await this.#tokenStorage.setToken(session.token);
        return session;
      },
      whoami: async () => {
        return this.#request<AuthSession>("GET", "/auth/session", undefined, true);
      },
      logout: async () => {
        await this.#tokenStorage.clearToken();
      }
    };

    this.pr = {
      create: async (input) => {
        return this.#request<PullRequestRecord>("POST", "/pr/create", input, true);
      }
    };

    this.stream = {
      subscribeToRepo: async ({ owner, repo }) => {
        const token = await this.#tokenStorage.getToken();
        if (!token) {
          throw new Error("Not authenticated. Run login first.");
        }

        const streamUrl = new URL("/stream", this.#baseUrl);
        streamUrl.searchParams.set("owner", owner);
        streamUrl.searchParams.set("repo", repo);
        streamUrl.searchParams.set("token", token);

        const socket = new this.#webSocketImpl(streamUrl);
        const subscription = new StreamSubscription(socket);

        socket.addEventListener("open", () => subscription.emit("open"));
        socket.addEventListener("close", () => subscription.emit("close"));
        socket.addEventListener("error", (error) => subscription.emit("error", error));
        socket.addEventListener("message", (event) => {
          try {
            const parsed = JSON.parse(String(event.data)) as StreamMessage;
            subscription.emit("event", parsed.event);
            subscription.emit(parsed.event.type, parsed.event);
          } catch (error) {
            subscription.emit("error", new Error(`Invalid stream payload: ${String(error)}`));
          }
        });

        await waitForSocketOpen(socket);
        return subscription;
      }
    };
  }

  async #request<T>(
    method: "GET" | "POST",
    path: string,
    body: unknown,
    authenticated: boolean
  ): Promise<T> {
    const headers: Record<string, string> = {
      "content-type": "application/json"
    };

    if (authenticated) {
      const token = await this.#tokenStorage.getToken();
      if (!token) {
        throw new Error("Not authenticated. Run login first.");
      }
      headers.authorization = `Bearer ${token}`;
    }

    const response = await this.#fetchImpl(new URL(path, this.#baseUrl), {
      method,
      headers,
      body: method === "GET" ? undefined : JSON.stringify(body ?? {})
    });

    if (!response.ok) {
      const errorBody = await response.text();
      throw new Error(`Request failed (${response.status}): ${errorBody}`);
    }

    return (await response.json()) as T;
  }
}

async function waitForSocketOpen(socket: WebSocket): Promise<void> {
  if (socket.readyState === WebSocket.OPEN) {
    return;
  }

  await new Promise<void>((resolve, reject) => {
    const onOpen = () => {
      cleanup();
      resolve();
    };
    const onError = (error: unknown) => {
      cleanup();
      reject(error);
    };
    const cleanup = () => {
      socket.removeEventListener("open", onOpen);
      socket.removeEventListener("error", onError);
    };

    socket.addEventListener("open", onOpen, { once: true });
    socket.addEventListener("error", onError, { once: true });
  });
}

export function createSdk(options: GoddardSdkOptions): GoddardSdk {
  return new GoddardSdk(options);
}

export type {
  AuthSession,
  CreatePrInput,
  DeviceFlowComplete,
  DeviceFlowSession,
  DeviceFlowStart,
  PiAgentConfig,
  PullRequestRecord,
  RepoEvent,
  RepoRef,
  ThinkingLevel,
  TokenStorage,
  GitHubWebhookInput
};
export { InMemoryTokenStorage };
export { LOOP_SYSTEM_PROMPT, SPEC_SYSTEM_PROMPT, PROPOSE_SYSTEM_PROMPT } from "./prompts.ts";
