import { createSdk, LOOP_SYSTEM_PROMPT, SPEC_SYSTEM_PROMPT, PROPOSE_SYSTEM_PROMPT } from "@goddard-ai/sdk";
import { inferRepoFromGitConfig, splitRepo } from "./git.ts";
import { FileTokenStorage } from "./storage.ts";
import { spawnSync } from "node:child_process";
import { command, runSafely, string, option, subcommands, restPositionals, binary } from "cmd-ts";

export type CliIo = {
  stdout: (line: string) => void;
  stderr: (line: string) => void;
};

type SdkClient = ReturnType<typeof createSdk>;

/**
 * Spawns the `pi` binary with the given args and returns its exit code.
 * Uses stdio: "inherit" so the interactive TUI flows directly to the terminal.
 */
function defaultSpawnPi(args: string[]): number {
  const result = spawnSync("pi", args, { stdio: "inherit" });
  return result.status ?? 1;
}

export type CliDeps = {
  createSdkClient?: (baseUrl: string) => SdkClient;
  /**
   * Injectable pi spawner for testing. Defaults to spawning the `pi` binary.
   * Receives the full argv array (everything after "pi") and returns the exit code.
   */
  spawnPi?: (args: string[]) => number;
};

export async function runCli(argv: string[], io: CliIo = defaultIo, deps: CliDeps = {}): Promise<number> {
  const getSdk = (baseUrlOpt?: string) => {
    const baseUrl = baseUrlOpt ?? process.env.GODDARD_BASE_URL ?? "http://127.0.0.1:8787";
    return deps.createSdkClient?.(baseUrl) ?? createSdk({ baseUrl, tokenStorage: new FileTokenStorage() });
  };
  const spawnPi = deps.spawnPi ?? defaultSpawnPi;

  const loginCmd = command({
    name: "login",
    args: {
      username: option({ type: string, long: "username" }),
      baseUrl: option({ type: string, long: "base-url", defaultValue: () => "" })
    },
    handler: async (args) => {
      try {
        const sdk = getSdk(args.baseUrl || undefined);
        const session = await sdk.auth.startDeviceFlow({ githubUsername: args.username });
        const auth = await sdk.auth.completeDeviceFlow({
          deviceCode: session.deviceCode,
          githubUsername: args.username
        });
        io.stdout(`Logged in as @${auth.githubUsername}`);
        return 0;
      } catch (e) {
        io.stderr(e instanceof Error ? e.message : String(e));
        return 1;
      }
    }
  });

  const logoutCmd = command({
    name: "logout",
    args: {
      baseUrl: option({ type: string, long: "base-url", defaultValue: () => "" })
    },
    handler: async (args) => {
      try {
        const sdk = getSdk(args.baseUrl || undefined);
        await sdk.auth.logout();
        io.stdout("Logged out");
        return 0;
      } catch (e) {
        io.stderr(e instanceof Error ? e.message : String(e));
        return 1;
      }
    }
  });

  const whoamiCmd = command({
    name: "whoami",
    args: {
      baseUrl: option({ type: string, long: "base-url", defaultValue: () => "" })
    },
    handler: async (args) => {
      try {
        const sdk = getSdk(args.baseUrl || undefined);
        const session = await sdk.auth.whoami();
        io.stdout(`@${session.githubUsername} (id:${session.githubUserId})`);
        return 0;
      } catch (e) {
        io.stderr(e instanceof Error ? e.message : String(e));
        return 1;
      }
    }
  });

  const prCreateCmd = command({
    name: "create",
    args: {
      title: option({ type: string, long: "title" }),
      body: option({ type: string, long: "body", defaultValue: () => undefined as any }),
      head: option({ type: string, long: "head", defaultValue: () => "main" }),
      base: option({ type: string, long: "base", defaultValue: () => "main" }),
      repo: option({ type: string, long: "repo", defaultValue: () => undefined as any }),
      baseUrl: option({ type: string, long: "base-url", defaultValue: () => "" })
    },
    handler: async (args) => {
      try {
        const sdk = getSdk(args.baseUrl || undefined);
        const repoRef = await resolveRepoRef(args.repo);
        const { owner, repo } = splitRepo(repoRef);
        const pr = await sdk.pr.create({
          owner,
          repo,
          title: args.title,
          body: args.body,
          head: args.head,
          base: args.base
        });
        io.stdout(`PR #${pr.number} created: ${pr.url}`);
        return 0;
      } catch (e) {
        io.stderr(e instanceof Error ? e.message : String(e));
        return 1;
      }
    }
  });

  const prCmd = subcommands({
    name: "pr",
    cmds: { create: prCreateCmd }
  });

  const streamCmd = command({
    name: "stream",
    args: {
      repo: option({ type: string, long: "repo", defaultValue: () => undefined as any }),
      baseUrl: option({ type: string, long: "base-url", defaultValue: () => "" })
    },
    handler: async (args) => {
      try {
        const sdk = getSdk(args.baseUrl || undefined);
        const repoRef = await resolveRepoRef(args.repo);
        const { owner, repo } = splitRepo(repoRef);
        const sub = await sdk.stream.subscribeToRepo({ owner, repo });

        io.stdout(`Streaming ${owner}/${repo}. Press Ctrl+C to exit.`);
        sub.on("event", (payload) => {
          io.stdout(JSON.stringify(payload));
        });

        await new Promise<void>((resolve) => {
          process.on("SIGINT", () => {
            sub.close();
            resolve();
          });
        });
        return 0;
      } catch (e) {
        io.stderr(e instanceof Error ? e.message : String(e));
        return 1;
      }
    }
  });

  const specCmd = command({
    name: "spec",
    args: {},
    handler: async () => {
      try {
        const exitCode = spawnPi(["--system-prompt", SPEC_SYSTEM_PROMPT]);
        return exitCode;
      } catch (e) {
        io.stderr(e instanceof Error ? e.message : String(e));
        return 1;
      }
    }
  });

  const proposeCmd = command({
    name: "propose",
    args: {
      prompt: restPositionals({ type: string, displayName: "prompt" })
    },
    handler: async (args) => {
      try {
        const exitCode = spawnPi(["--system-prompt", PROPOSE_SYSTEM_PROMPT, ...args.prompt]);
        return exitCode;
      } catch (e) {
        io.stderr(e instanceof Error ? e.message : String(e));
        return 1;
      }
    }
  });

  const agentsInitCmd = command({
    name: "init",
    args: {},
    handler: async () => {
      try {
        const sdk = getSdk();
        const agentsPath = await sdk.agents.appendSpecInstructions(process.cwd());
        io.stdout(`Updated agents configuration at ${agentsPath}`);
        return 0;
      } catch (e) {
        io.stderr(e instanceof Error ? e.message : String(e));
        return 1;
      }
    }
  });

  const agentsCmd = subcommands({
    name: "agents",
    cmds: { init: agentsInitCmd }
  });

  const app = subcommands({
    name: "goddard",
    cmds: {
      login: loginCmd,
      logout: logoutCmd,
      whoami: whoamiCmd,
      pr: prCmd,
      stream: streamCmd,
      spec: specCmd,
      propose: proposeCmd,
      agents: agentsCmd
    }
  });

  const res = await runSafely(app, argv);
  if (res._tag === "error") {
    io.stderr(res.error.config.message);
    return res.error.config.exitCode;
  }

  if (typeof res.value === "number") {
    return res.value;
  }
  if (res.value && typeof (res.value as any).value === "number") {
    return (res.value as any).value;
  }
  return 0;
}

const defaultIo: CliIo = {
  stdout: (line) => process.stdout.write(`${line}\n`),
  stderr: (line) => process.stderr.write(`${line}\n`)
};

async function resolveRepoRef(inputRepo?: string): Promise<string> {
  if (inputRepo) {
    return inputRepo;
  }

  const inferred = await inferRepoFromGitConfig();
  if (!inferred) {
    throw new Error("Unable to infer repository. Pass --repo owner/repo.");
  }

  return inferred;
}
