import test from "node:test";
import assert from "node:assert/strict";
import { runCli } from "../src/index.ts";
import { createSdk } from "@goddard-ai/sdk";

test("login command prints authenticated user", async () => {
  const lines: string[] = [];

  const sdk = createMockSdk({
    auth: {
      startDeviceFlow: async () => ({
        deviceCode: "dev_1",
        userCode: "ABCD",
        verificationUri: "https://github.com/login/device",
        expiresIn: 900,
        interval: 5
      }),
      completeDeviceFlow: async () => ({
        token: "tok_1",
        githubUsername: "alec",
        githubUserId: 42
      })
    }
  });

  const code = await runCli(
    ["login", "--username", "alec", "--base-url", "http://localhost:8787"],
    { stdout: (line) => lines.push(line), stderr: (line) => lines.push(`ERR:${line}`) },
    { createSdkClient: () => sdk }
  );

  assert.equal(code, 0);
  assert.equal(lines[0], "Logged in as @alec");
});

test("pr create command formats output", async () => {
  const lines: string[] = [];

  const sdk = createMockSdk({
    pr: {
      create: async () => ({
        id: 1,
        number: 1,
        owner: "goddard-ai",
        repo: "sdk",
        title: "Fix stream",
        body: "",
        head: "feat/stream",
        base: "main",
        url: "https://github.com/goddard-ai/sdk/pull/1",
        createdBy: "alec",
        createdAt: new Date().toISOString()
      })
    }
  });

  const code = await runCli(
    [
      "pr",
      "create",
      "--repo",
      "goddard-ai/sdk",
      "--title",
      "Fix stream",
      "--head",
      "feat/stream",
      "--base",
      "main"
    ],
    { stdout: (line) => lines.push(line), stderr: (line) => lines.push(`ERR:${line}`) },
    { createSdkClient: () => sdk }
  );

  assert.equal(code, 0);
  assert.equal(lines[0], "PR #1 created: https://github.com/goddard-ai/sdk/pull/1");
});

test("unknown command prints help and exits 1", async () => {
  const lines: string[] = [];

  const code = await runCli(
    ["unknown"],
    { stdout: (line) => lines.push(line), stderr: (line) => lines.push(`ERR:${line}`) },
    { createSdkClient: () => createMockSdk({}) }
  );

  assert.equal(code, 1);
  assert.equal(lines[0], "goddard commands:");
});

type SdkClient = ReturnType<typeof createSdk>;

type PartialSdk = {
  auth?: Partial<SdkClient["auth"]>;
  pr?: Partial<SdkClient["pr"]>;
  actions?: Partial<SdkClient["actions"]>;
  stream?: Partial<SdkClient["stream"]>;
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
      ...partial.pr
    },
    actions: {
      trigger: async () => {
        throw new Error("not mocked");
      },
      ...partial.actions
    },
    stream: {
      subscribeToRepo: async () => {
        throw new Error("not mocked");
      },
      ...partial.stream
    }
  } as SdkClient;
}
