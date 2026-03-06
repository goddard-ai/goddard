import test from "node:test";
import assert from "node:assert/strict";
import { runDaemonCli } from "../src/index.ts";
import { createSdk, type RepoEvent } from "@goddard-ai/sdk";

type SdkClient = ReturnType<typeof createSdk>;

test("daemon launches one-shot for managed PR comment event", async () => {
  const lines: string[] = [];
  const oneShots: string[] = [];
  const subscription = new MockSubscription();

  const sdk = createMockSdk({
    pr: {
      isManaged: async () => true
    },
    stream: {
      subscribeToRepo: async () => subscription as any
    }
  });

  const code = await runDaemonCli(
    ["run", "--repo", "goddard-ai/sdk"],
    { stdout: (line) => lines.push(line), stderr: (line) => lines.push(`ERR:${line}`) },
    {
      createSdkClient: () => sdk,
      runOneShot: async ({ prompt }) => {
        oneShots.push(prompt);
        return 0;
      },
      waitForShutdown: async (close) => {
        subscription.emit("event", {
          type: "comment",
          owner: "goddard-ai",
          repo: "sdk",
          prNumber: 42,
          author: "reviewer",
          body: "please add tests",
          reactionAdded: "eyes",
          createdAt: new Date().toISOString()
        } satisfies RepoEvent);
        await flush();
        close();
      }
    }
  );

  assert.equal(code, 0);
  assert.equal(oneShots.length, 1);
  assert.match(oneShots[0]!, /goddard-ai\/sdk#42/);
  assert.ok(lines.some((line) => line.includes("Launching one-shot pi session")));
});

test("daemon ignores unmanaged PR feedback", async () => {
  const oneShots: string[] = [];
  const subscription = new MockSubscription();

  const sdk = createMockSdk({
    pr: {
      isManaged: async () => false
    },
    stream: {
      subscribeToRepo: async () => subscription as any
    }
  });

  const code = await runDaemonCli(
    ["run", "--repo", "goddard-ai/sdk"],
    { stdout: () => {}, stderr: () => {} },
    {
      createSdkClient: () => sdk,
      runOneShot: async ({ prompt }) => {
        oneShots.push(prompt);
        return 0;
      },
      waitForShutdown: async (close) => {
        subscription.emit("event", {
          type: "review",
          owner: "goddard-ai",
          repo: "sdk",
          prNumber: 77,
          author: "reviewer",
          state: "changes_requested",
          body: "please adjust architecture",
          reactionAdded: "eyes",
          createdAt: new Date().toISOString()
        } satisfies RepoEvent);
        await flush();
        close();
      }
    }
  );

  assert.equal(code, 0);
  assert.equal(oneShots.length, 0);
});

class MockSubscription {
  readonly #handlers = new Map<string, Array<(payload?: unknown) => void>>();

  on(eventName: string, handler: (payload?: unknown) => void): this {
    const handlers = this.#handlers.get(eventName) ?? [];
    handlers.push(handler);
    this.#handlers.set(eventName, handlers);
    return this;
  }

  close(): void {
    // no-op for tests
  }

  emit(eventName: string, payload: unknown): void {
    for (const handler of this.#handlers.get(eventName) ?? []) {
      void handler(payload);
    }
  }
}

type PartialSdk = {
  auth?: Partial<SdkClient["auth"]>;
  pr?: Partial<SdkClient["pr"]>;
  stream?: Partial<SdkClient["stream"]>;
  agents?: Partial<SdkClient["agents"]>;
};

function createMockSdk(partial: PartialSdk): SdkClient {
  return {
    auth: {
      startDeviceFlow: async () => ({
        deviceCode: "dev_default",
        userCode: "USER",
        verificationUri: "https://github.com/login/device",
        expiresIn: 900,
        interval: 5
      }),
      completeDeviceFlow: async () => ({ token: "tok", githubUsername: "dev", githubUserId: 1 }),
      whoami: async () => ({ token: "tok", githubUsername: "dev", githubUserId: 1 }),
      logout: async () => undefined,
      ...partial.auth
    },
    pr: {
      create: async () => {
        throw new Error("not mocked");
      },
      isManaged: async () => false,
      ...partial.pr
    },
    stream: {
      subscribeToRepo: async () => {
        throw new Error("not mocked");
      },
      ...partial.stream
    },
    agents: {
      appendSpecInstructions: async () => {
        throw new Error("not mocked");
      },
      ...partial.agents
    }
  } as unknown as SdkClient;
}

async function flush(): Promise<void> {
  await new Promise<void>((resolve) => setImmediate(resolve));
}
