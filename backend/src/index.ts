import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import { randomUUID } from "node:crypto";
import {
  AuthSession,
  CreatePrInput,
  DeviceFlowComplete,
  DeviceFlowSession,
  DeviceFlowStart,
  GitHubWebhookInput,
  PullRequestRecord,
  RepoEvent,
  DeviceFlowStartSchema,
  DeviceFlowCompleteSchema,
  CreatePrInputSchema,
  GitHubWebhookInputSchema
} from "@goddard-ai/shared";
import { WebSocketServer, type WebSocket } from "ws";
import { type BackendControlPlane, HttpError, assertRepo } from "./control-plane.ts";

type SessionRecord = AuthSession & { expiresAt: number };
type DeviceSessionRecord = { githubUsername: string; createdAt: number; expiresAt: number };

const DEVICE_FLOW_EXPIRES_IN_SECONDS = 900;
const DEVICE_FLOW_INTERVAL_SECONDS = 5;
const AUTH_SESSION_TTL_MS = 1000 * 60 * 60 * 24;
const MAX_JSON_BODY_BYTES = 1024 * 1024;

type StartServerOptions = {
  port?: number;
  host?: string;
};

export type BackendServer = {
  port: number;
  close: () => Promise<void>;
};

export class InMemoryBackendControlPlane implements BackendControlPlane {
  #deviceSessions = new Map<string, DeviceSessionRecord>();
  #authSessions = new Map<string, SessionRecord>();
  #pullRequests: PullRequestRecord[] = [];
  #streamsByRepo = new Map<string, Set<WebSocket>>();
  #nextPrId = 1;

  startDeviceFlow(input: DeviceFlowStart = {}): DeviceFlowSession {
    const githubUsername = input.githubUsername?.trim() || "developer";
    const deviceCode = `dev_${randomUUID()}`;
    const userCode = randomUUID().slice(0, 8).toUpperCase();
    const createdAt = Date.now();

    this.#deviceSessions.set(deviceCode, {
      githubUsername,
      createdAt,
      expiresAt: createdAt + DEVICE_FLOW_EXPIRES_IN_SECONDS * 1000
    });

    return {
      deviceCode,
      userCode,
      verificationUri: "https://github.com/login/device",
      expiresIn: DEVICE_FLOW_EXPIRES_IN_SECONDS,
      interval: DEVICE_FLOW_INTERVAL_SECONDS
    };
  }

  completeDeviceFlow(input: DeviceFlowComplete): AuthSession {
    const pending = this.#deviceSessions.get(input.deviceCode);
    if (!pending) {
      throw new HttpError(404, "Unknown device code");
    }

    if (pending.expiresAt <= Date.now()) {
      this.#deviceSessions.delete(input.deviceCode);
      throw new HttpError(410, "Device code expired");
    }

    const githubUsername = input.githubUsername.trim();
    if (!githubUsername) {
      throw new HttpError(400, "githubUsername is required");
    }

    const expiresAt = Date.now() + AUTH_SESSION_TTL_MS;
    const session: SessionRecord = {
      token: `tok_${randomUUID()}`,
      githubUsername,
      githubUserId: hashToInteger(githubUsername),
      expiresAt
    };

    this.#authSessions.set(session.token, session);
    this.#deviceSessions.delete(input.deviceCode);

    return toPublicSession(session);
  }

  getSession(token: string): AuthSession {
    const session = this.#authSessions.get(token);
    if (!session) {
      throw new HttpError(401, "Invalid token");
    }

    if (session.expiresAt <= Date.now()) {
      this.#authSessions.delete(token);
      throw new HttpError(401, "Session expired");
    }

    return toPublicSession(session);
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
  controlPlane: BackendControlPlane = new InMemoryBackendControlPlane(),
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
        controlPlane.addStreamSocket?.(repoKey, ws);
        ws.on("close", () => controlPlane.removeStreamSocket?.(repoKey, ws));
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
  controlPlane: BackendControlPlane,
  req: IncomingMessage,
  res: ServerResponse
): Promise<void> {
  const method = req.method ?? "GET";
  const requestUrl = new URL(req.url ?? "/", `http://${req.headers.host ?? "localhost"}`);

  try {
    if (method === "POST" && requestUrl.pathname === "/auth/device/start") {
      const body = await readJson<unknown>(req);
      const parsed = DeviceFlowStartSchema.parse(body);
      return writeJson(res, 200, await controlPlane.startDeviceFlow(parsed));
    }

    if (method === "POST" && requestUrl.pathname === "/auth/device/complete") {
      const body = await readJson<unknown>(req);
      const parsed = DeviceFlowCompleteSchema.parse(body);
      return writeJson(res, 200, await controlPlane.completeDeviceFlow(parsed));
    }

    if (method === "GET" && requestUrl.pathname === "/auth/session") {
      const token = readBearerToken(req);
      return writeJson(res, 200, await controlPlane.getSession(token));
    }

    if (method === "POST" && requestUrl.pathname === "/pr/create") {
      const token = readBearerToken(req);
      const body = await readJson<unknown>(req);
      const parsed = CreatePrInputSchema.parse(body);
      return writeJson(res, 200, await controlPlane.createPr(token, parsed));
    }

    if (method === "POST" && requestUrl.pathname === "/webhooks/github") {
      const body = await readJson<unknown>(req);
      const parsed = GitHubWebhookInputSchema.parse(body);
      return writeJson(res, 200, await controlPlane.handleGitHubWebhook(parsed));
    }
  } catch (error) {
    if (error && typeof error === "object" && "name" in error && error.name === "ZodError") {
      throw new HttpError(400, "Invalid JSON body format");
    }
    throw error;
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
  let totalSize = 0;

  for await (const chunk of req) {
    const part = typeof chunk === "string" ? Buffer.from(chunk) : chunk;
    totalSize += part.byteLength;

    if (totalSize > MAX_JSON_BODY_BYTES) {
      throw new HttpError(413, "Request body too large");
    }

    chunks.push(part);
  }

  const raw = Buffer.concat(chunks).toString("utf-8");
  if (!raw) {
    return {} as T;
  }

  try {
    return JSON.parse(raw) as T;
  } catch {
    throw new HttpError(400, "Invalid JSON body");
  }
}

function readBearerToken(req: IncomingMessage): string {
  const header = req.headers.authorization;
  if (!header || !header.startsWith("Bearer ")) {
    throw new HttpError(401, "Missing Bearer token");
  }
  return header.slice("Bearer ".length);
}

function handleHttpError(res: ServerResponse, error: unknown): void {
  const statusCode = error instanceof HttpError ? error.statusCode : 500;
  const message = error instanceof Error ? error.message : "Unknown error";
  writeJson(res, statusCode, { error: message });
}

function toPublicSession(session: SessionRecord): AuthSession {
  return {
    token: session.token,
    githubUsername: session.githubUsername,
    githubUserId: session.githubUserId
  };
}

function hashToInteger(value: string): number {
  let hash = 0;
  for (let i = 0; i < value.length; i += 1) {
    hash = (hash << 5) - hash + value.charCodeAt(i);
    hash |= 0;
  }
  return Math.abs(hash) + 1000;
}
