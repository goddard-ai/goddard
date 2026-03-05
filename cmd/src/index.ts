import { createSdk, SPEC_SYSTEM_PROMPT, PROPOSE_SYSTEM_PROMPT } from "@goddard-ai/sdk";
import { LOOP_SYSTEM_PROMPT } from "@goddard-ai/loop";
import { inferRepoFromGitConfig, inferPrNumberFromGit, splitRepo } from "./git.ts";
import { FileTokenStorage, getLocalConfigPath, getGlobalConfigPath, fileExists, resolveLoopConfigPath } from "@goddard-ai/storage";
import { spawnSync } from "node:child_process";
import { access, mkdir, writeFile } from "node:fs/promises";
import { constants as fsConstants } from "node:fs";
import * as p from "@clack/prompts";
import { dirname, join } from "node:path";
import { homedir } from "node:os";
import { createJiti } from "@mariozechner/jiti";
import { createLoop } from "@goddard-ai/loop";
import type { GoddardLoopConfig } from "@goddard-ai/loop";
import { command, runSafely, string, option, subcommands, restPositionals, flag } from "cmd-ts";

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
  createLoopRuntime?: (config: GoddardLoopConfig) => { start: () => Promise<void> };
  /**
   * Injectable git operation runner for testing.
   */
  execGit?: (command: string, args: string[]) => { status: number | null; stdout: string; stderr: string };
  /**
   * Injectable prompts for testing interactiveness.
   */
  promptCommitMessage?: () => Promise<string | symbol>;
  promptPushBranch?: () => Promise<boolean | symbol>;
};

export async function runCli(argv: string[], io: CliIo = defaultIo, deps: CliDeps = {}): Promise<number> {
  const getSdk = (baseUrlOpt?: string) => {
    const baseUrl = baseUrlOpt ?? process.env.GODDARD_BASE_URL ?? "http://127.0.0.1:8787";
    return deps.createSdkClient?.(baseUrl) ?? createSdk({ baseUrl, tokenStorage: new FileTokenStorage() });
  };
  const spawnPi = deps.spawnPi ?? defaultSpawnPi;
  const createLoopRuntime = deps.createLoopRuntime ?? createLoop;

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

  const prReplyCmd = command({
    name: "reply",
    args: {
      body: option({ type: string, long: "body" }),
      pr: option({ type: string, long: "pr", defaultValue: () => "" }),
      repo: option({ type: string, long: "repo", defaultValue: () => undefined as any }),
      baseUrl: option({ type: string, long: "base-url", defaultValue: () => "" })
    },
    handler: async (args) => {
      try {
        const sdk = getSdk(args.baseUrl || undefined);
        const repoRef = await resolveRepoRef(args.repo);
        const { owner, repo } = splitRepo(repoRef);

        let prNumber: number;
        if (args.pr) {
          prNumber = parseInt(args.pr, 10);
        } else {
          const inferred = inferPrNumberFromGit();
          if (!inferred) {
            throw new Error("Unable to infer PR number from current branch. Pass --pr <number>.");
          }
          prNumber = inferred;
        }

        await sdk.pr.reply({
          owner,
          repo,
          prNumber,
          body: args.body
        });
        io.stdout(`Reply posted to PR #${prNumber}`);
        return 0;
      } catch (e) {
        io.stderr(e instanceof Error ? e.message : String(e));
        return 1;
      }
    }
  });

  const prCmd = subcommands({
    name: "pr",
    cmds: { create: prCreateCmd, reply: prReplyCmd }
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
        const execGit = deps.execGit ?? ((cmd: string, args: string[]) => {
          const res = spawnSync("git", [cmd, ...args], { encoding: "utf-8" });
          if (res.error) throw res.error;
          return { status: res.status, stdout: res.stdout, stderr: res.stderr };
        });

        const statusRes = execGit("status", ["--porcelain"]);
        const lines = statusRes.stdout.split('\n').filter(Boolean);

        const hasStaged = lines.some(line => {
          const status = line.substring(0, 2);
          return status[0] !== ' ' && status[0] !== '?';
        });

        if (hasStaged) {
          io.stderr("Error: You have staged changes. Please commit or stash them first.");
          return 1;
        }

        let agentsPath = join(process.cwd(), "AGENTS.md");
        let currentDir = process.cwd();
        while (true) {
          const candidatePath = join(currentDir, "AGENTS.md");
          try {
            await access(candidatePath, fsConstants.F_OK);
            agentsPath = candidatePath;
            break;
          } catch {
            const nextDir = dirname(currentDir);
            if (nextDir === currentDir) break;
            currentDir = nextDir;
          }
        }

        const agentsStatusRes = execGit("status", ["--porcelain", agentsPath]);
        if (agentsStatusRes.stdout.trim() !== "") {
          io.stderr("Error: AGENTS.md has uncommitted changes. Please commit or stash them first.");
          return 1;
        }

        const sdk = getSdk();
        await sdk.agents.appendSpecInstructions(process.cwd());
        io.stdout(`Updated agents configuration at ${agentsPath}`);

        try {
          const diffRes = execGit("diff", ["--", agentsPath]);
          if (diffRes.stdout && diffRes.stdout.trim() !== "") {
            io.stdout(diffRes.stdout);
          }
        } catch (e) {
          // Ignore diff errors
        }

        const promptCommitMessage = deps.promptCommitMessage ?? (async () => {
          return p.text({
            message: "Commit message (clear the text to skip commit)",
            initialValue: "Configure Goddard agent specifications",
          });
        });

        const commitMessage = await promptCommitMessage();

        if (p.isCancel(commitMessage)) {
          io.stdout("Commit skipped.");
          return 0;
        }

        const msg = (commitMessage as string).trim();
        if (!msg) {
          io.stdout("Commit skipped.");
          return 0;
        }

        try {
          const addRes = execGit("add", [agentsPath]);
          if (addRes.status !== 0) throw new Error(addRes.stderr);
          const commitRes = execGit("commit", ["-m", msg]);
          if (commitRes.status !== 0) throw new Error(commitRes.stderr);
          io.stdout(`Committed changes: ${msg}`);
        } catch (e) {
          io.stderr("Failed to commit changes.");
          io.stderr(e instanceof Error ? e.message : String(e));
          return 1;
        }

        const promptPushBranch = deps.promptPushBranch ?? (async () => {
          return p.confirm({
            message: "Would you like to push the branch?",
            initialValue: true,
          });
        });

        const shouldPush = await promptPushBranch();

        if (p.isCancel(shouldPush)) {
          io.stdout("Push skipped.");
          return 0;
        }

        if (shouldPush) {
          try {
            const pushRes = execGit("push", []);
            if (pushRes.status !== 0) throw new Error(pushRes.stderr);
            io.stdout("Pushed changes successfully.");
          } catch (e) {
            io.stderr("Failed to push changes.");
            io.stderr(e instanceof Error ? e.message : String(e));
            return 1;
          }
        } else {
          io.stdout("Push skipped.");
        }

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

  const loopInitCmd = command({
    name: "init",
    args: {
      global: flag({ long: "global", short: "g" })
    },
    handler: async (args) => {
      try {
        const targetPath = args.global
          ? getGlobalConfigPath()
          : getLocalConfigPath();

        if (await fileExists(targetPath)) {
          io.stderr(`Config file already exists at ${targetPath}`);
          return 1;
        }

        await mkdir(dirname(targetPath), { recursive: true });
        await writeFile(targetPath, DEFAULT_LOOP_CONFIG_TEMPLATE, "utf-8");

        io.stdout(`Created configuration at ${targetPath}`);
        io.stdout("Next step: goddard loop run");
        return 0;
      } catch (e) {
        io.stderr(e instanceof Error ? e.message : String(e));
        return 1;
      }
    }
  });

  const loopRunCmd = command({
    name: "run",
    args: {},
    handler: async () => {
      try {
        const configPath = await resolveLoopConfigPath();
        if (!configPath) {
          io.stderr("Could not find config.ts in the current directory's .goddard/ folder or in ~/.goddard.");
          io.stderr("Run `goddard loop init` to create one.");
          return 1;
        }

        const jiti = createJiti(process.cwd());
        const module = await jiti.import(configPath);
        const config = (module as any).default ?? module;
        if (!config) {
          io.stderr("Config file must export a default configuration object.");
          return 1;
        }

        const loop = createLoopRuntime(config as GoddardLoopConfig);
        await loop.start();
        io.stdout("Loop completed after DONE signal.");
        return 0;
      } catch (e) {
        io.stderr(e instanceof Error ? e.message : String(e));
        return 1;
      }
    }
  });

  const loopGenerateSystemdCmd = command({
    name: "generate-systemd",
    args: {
      global: flag({ long: "global", short: "g" })
    },
    handler: async (args) => {
      try {
        const configPath = args.global
          ? getGlobalConfigPath()
          : getLocalConfigPath();

        if (!(await fileExists(configPath))) {
          io.stderr(`Could not find config at ${configPath}`);
          return 1;
        }

        const jiti = createJiti(process.cwd());
        const module = await jiti.import(configPath);
        const config = ((module as any).default ?? module) as GoddardLoopConfig;

        const targetRoot = args.global ? homedir() : process.cwd();
        const outputPath = join(targetRoot, "systemd", "goddard.service");

        const user = config.systemd?.user ?? process.env.USER ?? "root";
        const workingDir = config.systemd?.workingDir ?? process.cwd();
        const restartSec = config.systemd?.restartSec ?? 10;
        const nice = config.systemd?.nice ?? 10;
        const environment = renderSystemdEnvironment(config.systemd?.environment);

        const service = `[Unit]\nDescription=Goddard Autonomous Agent Loop\nAfter=network.target\n\n[Service]\nType=simple\nUser=${user}\nWorkingDirectory=${workingDir}\nExecStart=goddard loop run\nRestart=always\nRestartSec=${restartSec}\nNice=${nice}\n${environment}[Install]\nWantedBy=multi-user.target\n`;

        await mkdir(dirname(outputPath), { recursive: true });
        await writeFile(outputPath, service, "utf-8");
        io.stdout(`Created systemd service file at ${outputPath}`);
        return 0;
      } catch (e) {
        io.stderr(e instanceof Error ? e.message : String(e));
        return 1;
      }
    }
  });

  const loopCmd = subcommands({
    name: "loop",
    cmds: {
      init: loopInitCmd,
      run: loopRunCmd,
      "generate-systemd": loopGenerateSystemdCmd
    }
  });

  const app = subcommands({
    name: "goddard",
    cmds: {
      login: loginCmd,
      logout: logoutCmd,
      whoami: whoamiCmd,
      pr: prCmd,
      loop: loopCmd,
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

function renderSystemdEnvironment(environment?: Record<string, string | undefined>): string {
  if (!environment) {
    return "";
  }

  const lines = Object.entries(environment)
    .filter(([, value]) => value !== undefined)
    .map(([key, value]) => `Environment=${key}=${quoteSystemdValue(value as string)}`);

  return lines.length > 0 ? `${lines.join("\n")}\n` : "";
}

function quoteSystemdValue(value: string): string {
  return `"${value.replace(/\\/g, "\\\\").replace(/"/g, "\\\"")}"`;
}

const DEFAULT_LOOP_CONFIG_TEMPLATE = `import { Models, defineConfig } from "@goddard-ai/config";

export default defineConfig({
  agent: {
    model: Models.Anthropic.ClaudeSonnet45,
    projectDir: "./",
    thinkingLevel: "low"
  },
  strategy: {
    nextPrompt: ({ cycleNumber, lastSummary }) =>
      \`Cycle \${cycleNumber}. Last summary: \${lastSummary ?? "none"}. Make one safe improvement, then answer with SUMMARY|DONE when ready.\`
  },
  rateLimits: {
    cycleDelay: "30m",
    maxTokensPerCycle: 128000,
    maxOpsPerMinute: 120,
    maxCyclesBeforePause: 100
  },
  retries: {
    maxAttempts: 3,
    initialDelayMs: 1000,
    maxDelayMs: 30000,
    backoffFactor: 2,
    jitterRatio: 0.2,
    retryableErrors: () => true
  },
  metrics: {
    enableLogging: true
  }
});
`;
