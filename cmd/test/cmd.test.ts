import test from "node:test";
import assert from "node:assert/strict";
import { runCli } from "../src/index.ts";
import { createSdk, SPEC_SYSTEM_PROMPT, PROPOSE_SYSTEM_PROMPT } from "@goddard-ai/sdk";

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
  assert.ok(lines[0]?.includes("ERR:error: found 1 error"));
});

test("spec command spawns pi with SPEC_SYSTEM_PROMPT and exits 0", async () => {
  const spawnCalls: Array<{ cmd: string; args: string[] }> = [];

  const code = await runCli(
    ["spec"],
    { stdout: () => {}, stderr: () => {} },
    {
      createSdkClient: () => createMockSdk({}),
      spawnPi: (args) => {
        spawnCalls.push({ cmd: "pi", args });
        return 0;
      }
    }
  );

  assert.equal(code, 0);
  assert.equal(spawnCalls.length, 1);
  assert.equal(spawnCalls[0]!.args[0], "--system-prompt");
  assert.equal(spawnCalls[0]!.args[1], SPEC_SYSTEM_PROMPT);
});

test("spec command propagates non-zero exit from pi", async () => {
  const code = await runCli(
    ["spec"],
    { stdout: () => {}, stderr: () => {} },
    {
      createSdkClient: () => createMockSdk({}),
      spawnPi: () => 1
    }
  );

  assert.equal(code, 1);
});

test("propose command spawns pi with PROPOSE_SYSTEM_PROMPT and passes args", async () => {
  const spawnCalls: Array<{ cmd: string; args: string[] }> = [];

  const code = await runCli(
    ["propose", "add auth"],
    { stdout: () => {}, stderr: () => {} },
    {
      createSdkClient: () => createMockSdk({}),
      spawnPi: (args) => {
        spawnCalls.push({ cmd: "pi", args });
        return 0;
      }
    }
  );

  assert.equal(code, 0);
  assert.equal(spawnCalls.length, 1);
  assert.equal(spawnCalls[0]!.args[0], "--system-prompt");
  assert.equal(spawnCalls[0]!.args[1], PROPOSE_SYSTEM_PROMPT);
  assert.equal(spawnCalls[0]!.args[2], "add auth");
});

type SdkClient = ReturnType<typeof createSdk>;

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
  } as SdkClient;
}

test("agents init command calls sdk.agents.appendSpecInstructions and handles commit/push", async () => {
  const lines: string[] = [];
  const execGitCalls: { cmd: string; args: string[] }[] = [];

  const sdk = createMockSdk({
    agents: {
      appendSpecInstructions: async (cwd?: string) => `${cwd ?? "mock"}/AGENTS.md`
    }
  });

  const code = await runCli(
    ["agents", "init"],
    { stdout: (line) => lines.push(line), stderr: (line) => lines.push(`ERR:${line}`) },
    {
      createSdkClient: () => sdk,
      execGit: (cmd, args) => {
        execGitCalls.push({ cmd, args });
        return { status: 0, stdout: "", stderr: "" };
      },
      promptCommitMessage: async () => "My custom config",
      promptPushBranch: async () => true,
    }
  );

  assert.equal(code, 0);
  assert.equal(lines[0], `Updated agents configuration at ${process.cwd()}/AGENTS.md`);
  assert.equal(lines[1], "Committed changes: My custom config");
  assert.equal(lines[2], "Pushed changes successfully.");

  assert.equal(execGitCalls.length, 6); // status (all), status (AGENTS.md), diff, add, commit, push
  assert.deepEqual(execGitCalls[3], { cmd: "add", args: [`${process.cwd()}/AGENTS.md`] });
  assert.deepEqual(execGitCalls[4], { cmd: "commit", args: ["-m", "My custom config"] });
  assert.deepEqual(execGitCalls[5], { cmd: "push", args: [] });
});

test("agents init command allows skipping commit", async () => {
  const lines: string[] = [];
  const execGitCalls: { cmd: string; args: string[] }[] = [];

  const sdk = createMockSdk({
    agents: {
      appendSpecInstructions: async (cwd?: string) => `${cwd ?? "mock"}/AGENTS.md`
    }
  });

  const code = await runCli(
    ["agents", "init"],
    { stdout: (line) => lines.push(line), stderr: (line) => lines.push(`ERR:${line}`) },
    {
      createSdkClient: () => sdk,
      execGit: (cmd, args) => {
        execGitCalls.push({ cmd, args });
        return { status: 0, stdout: "", stderr: "" };
      },
      promptCommitMessage: async () => "",
      promptPushBranch: async () => true,
    }
  );

  assert.equal(code, 0);
  assert.equal(lines[0], `Updated agents configuration at ${process.cwd()}/AGENTS.md`);
  assert.equal(lines[1], "Commit skipped.");

  assert.equal(execGitCalls.length, 3); // status, status, diff
});

test("agents init command allows skipping push", async () => {
  const lines: string[] = [];
  const execGitCalls: { cmd: string; args: string[] }[] = [];

  const sdk = createMockSdk({
    agents: {
      appendSpecInstructions: async (cwd?: string) => `${cwd ?? "mock"}/AGENTS.md`
    }
  });

  const code = await runCli(
    ["agents", "init"],
    { stdout: (line) => lines.push(line), stderr: (line) => lines.push(`ERR:${line}`) },
    {
      createSdkClient: () => sdk,
      execGit: (cmd, args) => {
        execGitCalls.push({ cmd, args });
        return { status: 0, stdout: "", stderr: "" };
      },
      promptCommitMessage: async () => "My custom config",
      promptPushBranch: async () => false,
    }
  );

  assert.equal(code, 0);
  assert.equal(lines[0], `Updated agents configuration at ${process.cwd()}/AGENTS.md`);
  assert.equal(lines[1], "Committed changes: My custom config");
  assert.equal(lines[2], "Push skipped.");

  assert.equal(execGitCalls.length, 5); // status, status, diff, add, commit
  assert.deepEqual(execGitCalls[3], { cmd: "add", args: [`${process.cwd()}/AGENTS.md`] });
  assert.deepEqual(execGitCalls[4], { cmd: "commit", args: ["-m", "My custom config"] });
});
