import { test } from "vitest";
import * as assert from "node:assert/strict";
import { runDaemonCli, type DaemonIo, type DaemonDeps } from "../src/index.ts";
import { createSdk, type RepoEvent, type StreamSubscription } from "@goddard-ai/sdk";
import { Models } from "@goddard-ai/config";

const defaultIo: DaemonIo = {
  stdout: () => {},
  stderr: () => {}
};

class MockStreamSubscription {
  #handlers = new Map<string, ((payload?: any) => void)[]>();

  on(eventName: string, handler: (payload?: any) => void): this {
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

type SdkClient = ReturnType<typeof createSdk>;

type PartialSdk = {
  auth?: Partial<SdkClient["auth"]>;
  pr?: Partial<SdkClient["pr"]>;
  stream?: Partial<SdkClient["stream"]>;
  agents?: Partial<SdkClient["agents"]>;
  loop?: Partial<SdkClient["loop"]>;
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
      login: async () => ({ token: "tok", githubUsername: "dev", githubUserId: 1 }),
      logout: async () => undefined,
      ...partial.auth
    },
    pr: {
      create: async () => {
        throw new Error("not mocked");
      },
      isManaged: async () => false,
      reply: async () => ({ success: true }),
      ...partial.pr
    },
    stream: {
      subscribeToRepo: async () => {
        throw new Error("not mocked");
      },
      ...partial.stream
    },
    agents: {
      init: async () => {
        throw new Error("not mocked");
      },
      ...partial.agents
    },
    loop: {
      init: async () => {
        throw new Error("not mocked");
      },
      run: async () => {
        throw new Error("not mocked");
      },
      generateSystemdService: async () => {
        throw new Error("not mocked");
      },
      ...partial.loop
    },
    config: {
      models: Models
    }
  } as unknown as SdkClient;
}

test("daemon run command subscribes to repo and handles events", async () => {
  const subscription = new MockStreamSubscription();
  let subCalls = 0;

  const sdk = createMockSdk({
    stream: {
      subscribeToRepo: async () => {
        subCalls++;
        return subscription as unknown as StreamSubscription;
      }
    },
    pr: {
      isManaged: async () => true
    }
  });

  const runOneShotCalls: any[] = [];
  const deps: DaemonDeps = {
    createSdkClient: () => sdk,
    runOneShot: async (input) => {
      runOneShotCalls.push(input);
      return 0;
    },
    waitForShutdown: async (close) => {
      // simulate an event
      const event: RepoEvent = {
        type: "comment",
        owner: "test",
        repo: "repo",
        prNumber: 123,
        author: "alice",
        body: "fix it",
        reactionAdded: "eyes",
        createdAt: new Date().toISOString()
      };
      subscription.emit("event", event);
      // then shut down
      close();
    }
  };

  const exitCode = await runDaemonCli(["run", "--repo", "test/repo"], defaultIo, deps);
  assert.equal(exitCode, 0);
  assert.equal(subCalls, 1);
  assert.equal(runOneShotCalls.length, 1);
  assert.equal(runOneShotCalls[0].event.prNumber, 123);
});
