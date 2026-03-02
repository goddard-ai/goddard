import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import { randomUUID } from "node:crypto";
import type {
  ActionRunRecord,
  AuthSession,
  CreatePrInput,
  DeviceFlowComplete,
  DeviceFlowSession,
  DeviceFlowStart,
  GitHubWebhookInput,
  PullRequestRecord,
  RepoEvent,
  TriggerActionInput
} from "@goddard-ai/sdk";
import { WebSocketServer, type WebSocket } from "ws";

type SessionRecord = AuthSession;

type StartServerOptions = {
  port?: number;
  host?: string;
};

export type BackendServer = {
  port: number;
  close: () => Promise<void>;
};

export class InMemoryBackendControlPlane {
  #deviceSessions = new Map<string, { githubUsername: string; createdAt: number }>();
  #authSessions = new Map<string, SessionRecord>();
  #pullRequests: PullRequestRecord[] = [];
  #actionRuns: ActionRunRecord[] = [];
  #streamsByRepo = new Map<string, Set<WebSocket>>();
  #nextPrId = 1;
  #nextRunId = 1;

  startDeviceFlow(input: DeviceFlowStart = {}): DeviceFlowSession {
    const githubUsername = input.githubUsername?.trim() || "developer";
    const deviceCode = `dev_${randomUUID()}`;
    const userCode = randomUUID().slice(0, 8).toUpperCase();

    this.#deviceSessions.set(deviceCode, { githubUsername, createdAt: Date.now() });

    return {
      deviceCode,
      userCode,
      verificationUri: "https://github.com/login/device",
      expiresIn: 900,
      interval: 5
    };
  }

  completeDeviceFlow(input: DeviceFlowComplete): AuthSession {
    const pending = this.#deviceSessions.get(input.deviceCode);
    if (!pending) {
      throw new HttpError(404, "Unknown device code");
    }

    const githubUsername = input.githubUsername.trim();
    if (!githubUsername) {
      throw new HttpError(400, "githubUsername is required");
    }

    const session: AuthSession = {
      token: `tok_${randomUUID()}`,
      githubUsername,
      githubUserId: hashToInteger(githubUsername)
    };

    this.#authSessions.set(session.token, session);
    this.#deviceSessions.delete(input.deviceCode);

    return session;
  }

  getSession(token: string): AuthSession {
    const session = this.#authSessions.get(token);
    if (!session) {
      throw new HttpError(401, "Invalid token");
    }
    return session;
  }

  createPr(token: string, input: CreatePrInput): PullRequestRecord {
    const session = this.getSession(token);
    assertRepo(input.owner, input.repo);
    if (!input.title.trim()) {
      throw new HttpError(400, "title is required");
    }

    const prNumber = this.#pullRequests.length + 1;
    const body = `${input.body?.trim() ?? ""}\n\nAuthored via CLI by @${session.githubUsername}`.trim();

    const record: PullRequestRecord = {
      id: this.#nextPrId++,
      number: prNumber,
      owner: input.owner,
      repo: input.repo,
      title: input.title,
      body,
      head: input.head,
      base: input.base,
      url: `https://github.com/${input.owner}/${input.repo}/pull/${prNumber}`,
      createdBy: session.githubUsername,
      createdAt: new Date().toISOString()
    };

    this.#pullRequests.push(record);

    this.broadcast({
      type: "pr.created",
      owner: input.owner,
      repo: input.repo,
      prNumber: record.number,
      title: record.title,
      author: session.githubUsername,
      createdAt: record.createdAt
    });

    return record;
  }

  triggerAction(token: string, input: TriggerActionInput): ActionRunRecord {
    const session = this.getSession(token);
    assertRepo(input.owner, input.repo);
    if (!input.workflowId.trim()) {
      throw new HttpError(400, "workflowId is required");
    }

    const run: ActionRunRecord = {
      id: this.#nextRunId++,
      owner: input.owner,
      repo: input.repo,
      workflowId: input.workflowId,
      ref: input.ref,
      status: "queued",
      triggeredBy: session.githubUsername,
      createdAt: new Date().toISOString()
    };
    this.#actionRuns.push(run);

    this.broadcast({
      type: "action.triggered",
      owner: input.owner,
      repo: input.repo,
      workflowId: run.workflowId,
      runId: run.id,
      status: "queued",
      author: run.triggeredBy,
      createdAt: run.createdAt
    });

    return run;
  }

  handleGitHubWebhook(event: GitHubWebhookInput): RepoEvent {
    assertRepo(event.owner, event.repo);

    const createdAt = new Date().toISOString();
    const mapped: RepoEvent =
      event.type === "issue_comment"
        ? {
            type: "comment",
            owner: event.owner,
            repo: event.repo,
            prNumber: event.prNumber,
            author: event.author,
            body: event.body,
            reactionAdded: "eyes",
            createdAt
          }
        : {
            type: "review",
            owner: event.owner,
            repo: event.repo,
            prNumber: event.prNumber,
            author: event.author,
            state: event.state,
            body: event.body,
            reactionAdded: "eyes",
            createdAt
          };

    this.broadcast(mapped);
    return mapped;
  }

  addStreamSocket(repoKey: string, socket: WebSocket): void {
    const room = this.#streamsByRepo.get(repoKey) ?? new Set<WebSocket>();
    room.add(socket);
    this.#streamsByRepo.set(repoKey, room);
  }

  removeStreamSocket(repoKey: string, socket: WebSocket): void {
    const room = this.#streamsByRepo.get(repoKey);
    room?.delete(socket);
    if (room && room.size === 0) {
      this.#streamsByRepo.delete(repoKey);
    }
  }

  broadcast(event: RepoEvent): void {
    const repoKey = `${event.owner}/${event.repo}`;
    const sockets = this.#streamsByRepo.get(repoKey);
    if (!sockets) {
      return;
    }

    const payload = JSON.stringify({ event });
    for (const socket of sockets) {
      if (socket.readyState === socket.OPEN) {
        socket.send(payload);
      }
    }
  }
}

export async function startBackendServer(
  controlPlane = new InMemoryBackendControlPlane(),
  options: StartServerOptions = {}
): Promise<BackendServer> {
  const host = options.host ?? "127.0.0.1";
  const port = options.port ?? 8787;

  const httpServer = createServer(async (req, res) => {
    try {
      await handleHttpRequest(controlPlane, req, res);
    } catch (error) {
      handleHttpError(res, error);
    }
  });

  const wss = new WebSocketServer({ noServer: true });

  httpServer.on("upgrade", (request, socket, head) => {
    const requestUrl = new URL(request.url ?? "/", `http://${request.headers.host}`);
    if (requestUrl.pathname !== "/stream") {
      socket.destroy();
      return;
    }

    try {
      const owner = requestUrl.searchParams.get("owner") ?? "";
      const repo = requestUrl.searchParams.get("repo") ?? "";
      const token = requestUrl.searchParams.get("token") ?? "";
      assertRepo(owner, repo);
      controlPlane.getSession(token);

      const repoKey = `${owner}/${repo}`;

      wss.handleUpgrade(request, socket, head, (ws: WebSocket) => {
        controlPlane.addStreamSocket(repoKey, ws);
        ws.on("close", () => controlPlane.removeStreamSocket(repoKey, ws));
      });
    } catch {
      socket.destroy();
    }
  });

  await new Promise<void>((resolve) => httpServer.listen(port, host, () => resolve()));

  return {
    port: Number((httpServer.address() as { port: number }).port),
    close: async () => {
      for (const client of wss.clients) {
        client.close();
      }
      await new Promise<void>((resolve, reject) => {
        httpServer.close((error) => {
          if (error) {
            reject(error);
            return;
          }
          resolve();
        });
      });
    }
  };
}

async function handleHttpRequest(
  controlPlane: InMemoryBackendControlPlane,
  req: IncomingMessage,
  res: ServerResponse
): Promise<void> {
  const method = req.method ?? "GET";
  const requestUrl = new URL(req.url ?? "/", `http://${req.headers.host ?? "localhost"}`);

  if (method === "POST" && requestUrl.pathname === "/auth/device/start") {
    const body = await readJson<DeviceFlowStart>(req);
    return writeJson(res, 200, controlPlane.startDeviceFlow(body));
  }

  if (method === "POST" && requestUrl.pathname === "/auth/device/complete") {
    const body = await readJson<DeviceFlowComplete>(req);
    return writeJson(res, 200, controlPlane.completeDeviceFlow(body));
  }

  if (method === "GET" && requestUrl.pathname === "/auth/session") {
    const token = readBearerToken(req);
    return writeJson(res, 200, controlPlane.getSession(token));
  }

  if (method === "POST" && requestUrl.pathname === "/pr/create") {
    const token = readBearerToken(req);
    const body = await readJson<CreatePrInput>(req);
    return writeJson(res, 200, controlPlane.createPr(token, body));
  }

  if (method === "POST" && requestUrl.pathname === "/actions/trigger") {
    const token = readBearerToken(req);
    const body = await readJson<TriggerActionInput>(req);
    return writeJson(res, 200, controlPlane.triggerAction(token, body));
  }

  if (method === "POST" && requestUrl.pathname === "/webhooks/github") {
    const body = await readJson<GitHubWebhookInput>(req);
    return writeJson(res, 200, controlPlane.handleGitHubWebhook(body));
  }

  throw new HttpError(404, "Not found");
}

function writeJson(res: ServerResponse, statusCode: number, payload: unknown): void {
  res.statusCode = statusCode;
  res.setHeader("content-type", "application/json");
  res.end(JSON.stringify(payload));
}

async function readJson<T>(req: IncomingMessage): Promise<T> {
  const chunks: Uint8Array[] = [];
  for await (const chunk of req) {
    chunks.push(typeof chunk === "string" ? Buffer.from(chunk) : chunk);
  }

  const raw = Buffer.concat(chunks).toString("utf-8");
  if (!raw) {
    return {} as T;
  }

  return JSON.parse(raw) as T;
}

function readBearerToken(req: IncomingMessage): string {
  const header = req.headers.authorization;
  if (!header || !header.startsWith("Bearer ")) {
    throw new HttpError(401, "Missing Bearer token");
  }
  return header.slice("Bearer ".length);
}

function assertRepo(owner: string, repo: string): void {
  if (!owner.trim() || !repo.trim()) {
    throw new HttpError(400, "owner and repo are required");
  }
}

class HttpError extends Error {
  constructor(
    readonly statusCode: number,
    message: string
  ) {
    super(message);
  }
}

function handleHttpError(res: ServerResponse, error: unknown): void {
  const statusCode = error instanceof HttpError ? error.statusCode : 500;
  const message = error instanceof Error ? error.message : "Unknown error";
  writeJson(res, statusCode, { error: message });
}

function hashToInteger(value: string): number {
  let hash = 0;
  for (let i = 0; i < value.length; i += 1) {
    hash = (hash << 5) - hash + value.charCodeAt(i);
    hash |= 0;
  }
  return Math.abs(hash) + 1000;
}
