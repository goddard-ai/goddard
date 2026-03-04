import { createSdk, LOOP_SYSTEM_PROMPT, SPEC_SYSTEM_PROMPT, PROPOSE_SYSTEM_PROMPT } from "@goddard-ai/sdk";
import { inferRepoFromGitConfig, splitRepo } from "./git.ts";
import { FileTokenStorage, getLocalConfigPath, getGlobalConfigPath, fileExists, resolveLoopConfigPath } from "@goddard-ai/storage";
import { spawnSync } from "node:child_process";
import { access, mkdir, writeFile } from "node:fs/promises";
import { constants as fsConstants } from "node:fs";
import { dirname, join } from "node:path";
import { homedir } from "node:os";
import { createJiti } from "@mariozechner/jiti";
import { createLoop } from "./loop/index.ts";
import type { GoddardLoopConfig } from "./loop/index.ts";
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

  const prCmd = subcommands({
    name: "pr",
    cmds: { create: prCreateCmd }
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
          io.stderr("Could not find goddard.config.ts in the current directory or config.ts in ~/.goddard.");
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

const DEFAULT_LOOP_CONFIG_TEMPLATE = `export default {
  agent: {
    model: "anthropic/claude-sonnet-4-5",
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
};
`;
