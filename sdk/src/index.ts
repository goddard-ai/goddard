import {
  type AuthSession,
  type CreatePrInput,
  type DeviceFlowComplete,
  type DeviceFlowSession,
  type DeviceFlowStart,
  type GitHubWebhookInput,
  type PiAgentConfig,
  type PullRequestRecord,
  type RepoEvent,
  type RepoRef,
  type StreamMessage,
  type ThinkingLevel
} from "@goddard-ai/schema";
import * as routes from "@goddard-ai/schema/routes";
import { createClient, type RouteRequest } from "rouzer";
import { InMemoryTokenStorage, type TokenStorage } from "@goddard-ai/storage";
import { Models } from "@goddard-ai/config";

export const SDK_VERSION = "0.1.0";

type FetchLike = typeof fetch;
type RouzerHttpClient = ReturnType<typeof createClient<typeof routes>>;

export type GoddardSdkOptions = {
  baseUrl: string;
  tokenStorage?: TokenStorage;
  fetchImpl?: FetchLike;
};

type StreamHandler = (event?: unknown) => void;

export class StreamSubscription {
  #dispose: () => void;
  #listeners = new Map<string, Set<StreamHandler>>();
  #isClosed = false;

  constructor(dispose: () => void) {
    this.#dispose = dispose;
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
    if (this.#isClosed) {
      return;
    }

    this.#isClosed = true;
    this.#dispose();
    this.emit("close");
  }

  isClosed(): boolean {
    return this.#isClosed;
  }
}

export class GoddardSdk {
  readonly auth: {
    startDeviceFlow: (input?: DeviceFlowStart) => Promise<DeviceFlowSession>;
    completeDeviceFlow: (input: DeviceFlowComplete) => Promise<AuthSession>;
    login: (options: { githubUsername?: string; onPrompt: (verificationUri: string, userCode: string) => void }) => Promise<AuthSession>;
    whoami: () => Promise<AuthSession>;
    logout: () => Promise<void>;
  };

  readonly pr: {
    create: (input: CreatePrInput) => Promise<PullRequestRecord>;
    isManaged: (input: RepoRef & { prNumber: number }) => Promise<boolean>;
    reply: (input: RepoRef & { prNumber: number; body: string }) => Promise<{ success: boolean }>;
  };

  readonly stream: {
    subscribeToRepo: (repo: RepoRef) => Promise<StreamSubscription>;
  };

  readonly config: {
    models: typeof Models;
  };

  readonly #baseUrl: URL;
  readonly #tokenStorage: TokenStorage;
  readonly #fetchImpl: FetchLike;
  readonly #rouzerClient: RouzerHttpClient;

  constructor(options: GoddardSdkOptions) {
    this.#baseUrl = new URL(options.baseUrl);
    this.#tokenStorage = options.tokenStorage ?? new InMemoryTokenStorage();
    this.#fetchImpl = options.fetchImpl ?? fetch;
    this.#rouzerClient = createClient({
      baseURL: this.#baseUrl.toString(),
      fetch: this.#fetchImpl,
      routes: routes
    });

    this.auth = {
      startDeviceFlow: async (input = {}) => {
        return this.#sendJson<DeviceFlowSession>(
          this.#rouzerClient.request(routes.authDeviceStartRoute.POST({ body: input }))
        );
      },
      completeDeviceFlow: async (input) => {
        const session = await this.#sendJson<AuthSession>(
          this.#rouzerClient.request(routes.authDeviceCompleteRoute.POST({ body: input }))
        );
        await this.#tokenStorage.setToken(session.token);
        return session;
      },
      login: async ({ githubUsername, onPrompt }) => {
        const start = await this.auth.startDeviceFlow({ githubUsername });
        onPrompt(start.verificationUri, start.userCode);
        
        const expiresAt = Date.now() + start.expiresIn * 1000;
        let delay = start.interval * 1000;

        while (Date.now() < expiresAt) {
          try {
            return await this.auth.completeDeviceFlow({ deviceCode: start.deviceCode, githubUsername: githubUsername ?? "" });
          } catch (e: any) {
             // Continue polling if not an unexpected error
             if (e.message && !e.message.includes("authorization_pending") && !e.message.includes("slow_down")) {
                 throw e;
             }
             if (e.message && e.message.includes("slow_down")) {
                 delay += 5000;
             }
          }
          await new Promise((resolve) => setTimeout(resolve, delay));
        }
        throw new Error("Device flow authentication timed out.");
      },
      whoami: async () => {
        const token = await this.#requireToken();
        return this.#sendJson<AuthSession>(
          this.#rouzerClient.request(routes.authSessionRoute.GET({ headers: { authorization: `Bearer ${token}` } }))
        );
      },
      logout: async () => {
        await this.#tokenStorage.clearToken();
      }
    };

    this.pr = {
      create: async (input) => {
        const token = await this.#requireToken();
        return this.#sendJson<PullRequestRecord>(
          this.#rouzerClient.request(
            routes.prCreateRoute.POST({
              headers: { authorization: `Bearer ${token}` },
              body: input
            })
          )
        );
      },
      isManaged: async ({ owner, repo, prNumber }) => {
        const token = await this.#requireToken();
        const result = await this.#sendJson<{ managed: boolean }>(
          this.#rouzerClient.request(
            routes.prManagedRoute.GET({
              headers: { authorization: `Bearer ${token}` },
              query: { owner, repo, prNumber }
            })
          )
        );
        return result.managed;
      },
      reply: async (input) => {
        const token = await this.#requireToken();
        return this.#sendJson<{ success: boolean }>(
          this.#rouzerClient.request(
            routes.prReplyRoute.POST({
              headers: { authorization: `Bearer ${token}` },
              body: input
            })
          )
        );
      }
    };

    this.stream = {
      subscribeToRepo: async ({ owner, repo }) => {
        const token = await this.#tokenStorage.getToken();
        if (!token) {
          throw new Error("Not authenticated. Run login first.");
        }

        const streamRequest = routes.repoStreamRoute.GET({
          headers: { authorization: `Bearer ${token}` },
          query: { owner, repo }
        });
        const streamUrl = buildRouteUrl(this.#baseUrl, streamRequest);
        const abortController = new AbortController();

        const response = await this.#fetchImpl(streamUrl, {
          method: "GET",
          headers: {
            accept: "text/event-stream",
            authorization: `Bearer ${token}`
          },
          signal: abortController.signal
        });

        if (!response.ok) {
          throw new Error(`Stream request failed (${response.status}): ${await response.text()}`);
        }

        if (!response.body) {
          throw new Error("Stream response did not include a body");
        }

        const subscription = new StreamSubscription(() => {
          abortController.abort();
        });

        subscription.emit("open");
        void consumeSseResponse(response.body, subscription, abortController.signal);

        return subscription;
      }
    };

    this.config = {
      models: Models
    };
  }

  async #sendJson<T>(responsePromise: Promise<Response>): Promise<T> {
    const response = await responsePromise;

    if (!response.ok) {
      const errorBody = await response.text();
      throw new Error(`Request failed (${response.status}): ${errorBody}`);
    }

    return (await response.json()) as T;
  }

  async #requireToken(): Promise<string> {
    const token = await this.#tokenStorage.getToken();
    if (!token) {
      throw new Error("Not authenticated. Run login first.");
    }

    return token;
  }
}

function buildRouteUrl(baseUrl: URL, request: RouteRequest): URL {
  const pathname = request.path.href(request.args.path as Record<string, string> | undefined);
  const url = new URL(pathname, baseUrl);
  const query = request.args.query as Record<string, unknown> | undefined;

  if (query) {
    for (const [key, value] of Object.entries(query)) {
      if (value !== undefined) {
        url.searchParams.set(key, String(value));
      }
    }
  }

  return url;
}

async function consumeSseResponse(
  body: ReadableStream<Uint8Array>,
  subscription: StreamSubscription,
  signal: AbortSignal
): Promise<void> {
  const reader = body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";

  try {
    while (true) {
      const { value, done } = await reader.read();
      if (done) {
        break;
      }

      buffer += decoder.decode(value, { stream: true });
      buffer = flushSseBuffer(buffer, subscription);
    }

    buffer += decoder.decode();
    flushSseBuffer(buffer, subscription);
  } catch (error) {
    if (!signal.aborted) {
      subscription.emit("error", error);
    }
  } finally {
    await reader.cancel().catch(() => {});
    if (!subscription.isClosed()) {
      subscription.close();
    }
  }
}

function flushSseBuffer(buffer: string, subscription: StreamSubscription): string {
  let remaining = buffer;

  while (true) {
    const match = remaining.match(/\r?\n\r?\n/);
    if (!match || match.index === undefined) {
      return remaining;
    }

    const chunk = remaining.slice(0, match.index);
    remaining = remaining.slice(match.index + match[0].length);

    const data = parseSseData(chunk);
    if (!data) {
      continue;
    }

    try {
      const parsed = JSON.parse(data) as StreamMessage;
      subscription.emit("event", parsed.event);
      subscription.emit(parsed.event.type, parsed.event);
    } catch (error) {
      subscription.emit("error", new Error(`Invalid stream payload: ${String(error)}`));
    }
  }
}

function parseSseData(chunk: string): string | null {
  const lines = chunk.split(/\r?\n/);
  const dataLines: string[] = [];

  for (const line of lines) {
    if (!line || line.startsWith(":")) {
      continue;
    }

    if (line.startsWith("data:")) {
      dataLines.push(line.slice("data:".length).trimStart());
    }
  }

  return dataLines.length > 0 ? dataLines.join("\n") : null;
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
export { SPEC_SYSTEM_PROMPT, PROPOSE_SYSTEM_PROMPT } from "./prompts.ts";
export { LOOP_SYSTEM_PROMPT } from "@goddard-ai/loop";
export type { GoddardLoopConfig } from "@goddard-ai/loop";
