import { randomUUID } from "node:crypto";
import { createServer as createNodeServer } from "@hattip/adapter-node";
import {
  type AuthSession,
  type CreatePrInput,
  type DeviceFlowComplete,
  type DeviceFlowSession,
  type DeviceFlowStart,
  type GitHubWebhookInput,
  type PullRequestRecord,
  type RepoEvent
} from "@goddard-ai/schema";
import { type BackendControlPlane, HttpError, assertRepo } from "./control-plane.ts";
import { createBackendRouter } from "./router.ts";

type SessionRecord = AuthSession & { expiresAt: number };
type DeviceSessionRecord = { githubUsername: string; createdAt: number; expiresAt: number };
type StreamSink = {
  send: (payload: string) => void;
  close?: () => void;
};

const DEVICE_FLOW_EXPIRES_IN_SECONDS = 900;
const DEVICE_FLOW_INTERVAL_SECONDS = 5;
const AUTH_SESSION_TTL_MS = 1000 * 60 * 60 * 24;

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
  #streamsByRepo = new Map<string, Set<StreamSink>>();
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

  isManagedPr(owner: string, repo: string, prNumber: number): boolean {
    assertRepo(owner, repo);
    if (!Number.isInteger(prNumber) || prNumber <= 0) {
      throw new HttpError(400, "prNumber must be a positive integer");
    }

    return this.#pullRequests.some(
      (pullRequest) =>
        pullRequest.owner === owner && pullRequest.repo === repo && pullRequest.number === prNumber
    );
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

  addStreamSocket(repoKey: string, socket: unknown): void {
    if (!isStreamSink(socket)) {
      return;
    }

    const room = this.#streamsByRepo.get(repoKey) ?? new Set<StreamSink>();
    room.add(socket);
    this.#streamsByRepo.set(repoKey, room);
  }

  removeStreamSocket(repoKey: string, socket: unknown): void {
    if (!isStreamSink(socket)) {
      return;
    }

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
      try {
        socket.send(payload);
      } catch {
        sockets.delete(socket);
        socket.close?.();
      }
    }

    if (sockets.size === 0) {
      this.#streamsByRepo.delete(repoKey);
    }
  }
}

export async function startBackendServer(
  controlPlane: BackendControlPlane = new InMemoryBackendControlPlane(),
  options: StartServerOptions = {}
): Promise<BackendServer> {
  const host = options.host ?? "127.0.0.1";
  const port = options.port ?? 8787;

  const router = createBackendRouter({
    createControlPlane: () => controlPlane,
    broadcastToRepo: async (_env, _owner, _repo, event) => {
      broadcastToInMemoryStreams(controlPlane, event);
    },
    handleRepoStream: async (_env, owner, repo, request) => {
      const repoKey = `${owner}/${repo}`;
      const sseSession = createSseSession(() => {
        controlPlane.removeStreamSocket?.(repoKey, sseSession.sink);
      });

      controlPlane.addStreamSocket?.(repoKey, sseSession.sink);
      request.signal.addEventListener(
        "abort",
        () => {
          controlPlane.removeStreamSocket?.(repoKey, sseSession.sink);
          sseSession.sink.close?.();
        },
        { once: true }
      );

      return sseSession.response;
    }
  });

  const httpServer = createNodeServer(router);

  await new Promise<void>((resolve) => httpServer.listen(port, host, () => resolve()));

  return {
    port: Number((httpServer.address() as { port: number }).port),
    close: async () => {
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

function broadcastToInMemoryStreams(controlPlane: BackendControlPlane, event: RepoEvent): void {
  if ("broadcast" in controlPlane && typeof controlPlane.broadcast === "function") {
    controlPlane.broadcast(event);
  }
}

function isStreamSink(value: unknown): value is StreamSink {
  return !!value && typeof value === "object" && "send" in value && typeof (value as StreamSink).send === "function";
}

function createSseSession(onClose: () => void): { response: Response; sink: StreamSink } {
  const encoder = new TextEncoder();
  let controller: ReadableStreamDefaultController<Uint8Array> | null = null;
  let isClosed = false;

  const close = () => {
    if (isClosed) {
      return;
    }

    isClosed = true;
    try {
      controller?.close();
    } catch {
      // no-op: controller can already be closed by the runtime
    }
    onClose();
  };

  const stream = new ReadableStream<Uint8Array>({
    start(ctrl) {
      controller = ctrl;
      ctrl.enqueue(encoder.encode(": connected\n\n"));
    },
    cancel() {
      close();
    }
  });

  const sink: StreamSink = {
    send(payload) {
      if (isClosed || !controller) {
        return;
      }

      controller.enqueue(encoder.encode(formatSseDataFrame(payload)));
    },
    close
  };

  return {
    response: new Response(stream, {
      status: 200,
      headers: {
        "content-type": "text/event-stream",
        "cache-control": "no-cache, no-transform",
        connection: "keep-alive"
      }
    }),
    sink
  };
}

function formatSseDataFrame(payload: string): string {
  const normalized = payload.replace(/\r\n/g, "\n").replace(/\r/g, "\n");
  const lines = normalized.split("\n");
  return `${lines.map((line) => `data: ${line}`).join("\n")}\n\n`;
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
