import { createSdk } from "@goddard-ai/sdk";
import { inferRepoFromGitConfig, splitRepo } from "./git.ts";
import { FileTokenStorage } from "./storage.ts";

export type CliIo = {
  stdout: (line: string) => void;
  stderr: (line: string) => void;
};

type SdkClient = ReturnType<typeof createSdk>;

export type CliDeps = {
  createSdkClient?: (baseUrl: string) => SdkClient;
};

export async function runCli(argv: string[], io: CliIo = defaultIo, deps: CliDeps = {}): Promise<number> {
  const command = argv[0];
  const subcommand = argv[1];
  const options = parseOptions(argv);

  const baseUrl = options["base-url"] ?? process.env.GODDARD_BASE_URL ?? "http://127.0.0.1:8787";
  const sdk =
    deps.createSdkClient?.(baseUrl) ??
    createSdk({
      baseUrl,
      tokenStorage: new FileTokenStorage()
    });

  try {
    if (command === "login") {
      const username = requiredOption(options, "username");
      const session = await sdk.auth.startDeviceFlow({ githubUsername: username });
      const auth = await sdk.auth.completeDeviceFlow({
        deviceCode: session.deviceCode,
        githubUsername: username
      });
      io.stdout(`Logged in as @${auth.githubUsername}`);
      return 0;
    }

    if (command === "logout") {
      await sdk.auth.logout();
      io.stdout("Logged out");
      return 0;
    }

    if (command === "whoami") {
      const session = await sdk.auth.whoami();
      io.stdout(`@${session.githubUsername} (id:${session.githubUserId})`);
      return 0;
    }

    if (command === "pr" && subcommand === "create") {
      const repoRef = await resolveRepoRef(options["repo"]);
      const { owner, repo } = splitRepo(repoRef);
      const title = requiredOption(options, "title");
      const head = options.head ?? "main";
      const base = options.base ?? "main";
      const body = options.body;

      const pr = await sdk.pr.create({ owner, repo, title, body, head, base });
      io.stdout(`PR #${pr.number} created: ${pr.url}`);
      return 0;
    }

    if (command === "actions" && subcommand === "trigger") {
      const repoRef = await resolveRepoRef(options["repo"]);
      const { owner, repo } = splitRepo(repoRef);
      const workflowId = requiredOption(options, "workflow");
      const ref = options.ref ?? "main";

      const run = await sdk.actions.trigger({ owner, repo, workflowId, ref });
      io.stdout(`Action queued: run ${run.id} (${run.workflowId} on ${run.ref})`);
      return 0;
    }

    if (command === "stream") {
      const repoRef = await resolveRepoRef(options["repo"]);
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
    }

    printHelp(io);
    return 1;
  } catch (error) {
    io.stderr(error instanceof Error ? error.message : String(error));
    return 1;
  }
}

const defaultIo: CliIo = {
  stdout: (line) => process.stdout.write(`${line}\n`),
  stderr: (line) => process.stderr.write(`${line}\n`)
};

function parseOptions(args: string[]): Record<string, string> {
  const output: Record<string, string> = {};

  for (let i = 0; i < args.length; i += 1) {
    const arg = args[i];
    if (!arg.startsWith("--")) {
      continue;
    }
    const key = arg.slice(2);
    const value = args[i + 1];
    if (!value || value.startsWith("--")) {
      output[key] = "true";
      continue;
    }
    output[key] = value;
    i += 1;
  }

  return output;
}

function requiredOption(options: Record<string, string>, key: string): string {
  const value = options[key];
  if (!value) {
    throw new Error(`Missing required --${key}`);
  }
  return value;
}

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

function printHelp(io: CliIo): void {
  io.stdout("goddard commands:");
  io.stdout("  login --username <name> [--base-url <url>]");
  io.stdout("  logout [--base-url <url>]");
  io.stdout("  whoami [--base-url <url>]");
  io.stdout("  pr create --title <title> [--body <body>] [--head <branch>] [--base <branch>] [--repo owner/repo]");
  io.stdout("  actions trigger --workflow <workflow-id> [--ref <ref>] [--repo owner/repo]");
  io.stdout("  stream [--repo owner/repo] [--base-url <url>]");
}
