import { createSdk, type RepoEvent } from "@goddard-ai/sdk";
import { spawnSync } from "node:child_process";
import { command, option, runSafely, string, subcommands } from "cmd-ts";
import { FileTokenStorage } from "./storage.ts";

export type DaemonIo = {
  stdout: (line: string) => void;
  stderr: (line: string) => void;
};

type SdkClient = ReturnType<typeof createSdk>;

type OneShotInput = {
  event: Extract<RepoEvent, { type: "comment" | "review" }>;
  prompt: string;
  projectDir: string;
  piBin: string;
};

export type DaemonDeps = {
  createSdkClient?: (baseUrl: string) => SdkClient;
  runOneShot?: (input: OneShotInput) => Promise<number> | number;
  waitForShutdown?: (close: () => void) => Promise<void>;
};

export async function runDaemonCli(
  argv: string[],
  io: DaemonIo = defaultIo,
  deps: DaemonDeps = {}
): Promise<number> {
  const runCmd = command({
    name: "run",
    args: {
      repo: option({ type: string, long: "repo" }),
      projectDir: option({ type: string, long: "project-dir", defaultValue: () => process.cwd() }),
      baseUrl: option({ type: string, long: "base-url", defaultValue: () => "" }),
      piBin: option({ type: string, long: "pi-bin", defaultValue: () => "pi" })
    },
    handler: async (args) => {
      const baseUrl = args.baseUrl || process.env.GODDARD_BASE_URL || "http://127.0.0.1:8787";
      const sdk = deps.createSdkClient?.(baseUrl) ?? createSdk({ baseUrl, tokenStorage: new FileTokenStorage() });
      const runOneShot = deps.runOneShot ?? defaultRunOneShot;
      const waitForShutdown = deps.waitForShutdown ?? defaultWaitForShutdown;

      try {
        const { owner, repo } = splitRepo(args.repo);
        const runningPrs = new Set<number>();
        const subscription = await sdk.stream.subscribeToRepo({ owner, repo });

        io.stdout(`Daemon subscribed to ${owner}/${repo}. Waiting for PR feedback events...`);

        subscription.on("event", async (payload) => {
          const event = payload as RepoEvent;
          if (!isFeedbackEvent(event)) {
            return;
          }

          if (runningPrs.has(event.prNumber)) {
            io.stdout(`Skipping PR #${event.prNumber}; one-shot already running.`);
            return;
          }

          runningPrs.add(event.prNumber);
          try {
            const managed = await sdk.pr.isManaged({ owner: event.owner, repo: event.repo, prNumber: event.prNumber });
            if (!managed) {
              io.stdout(`Ignoring ${event.type} on unmanaged PR #${event.prNumber}.`);
              return;
            }

            const prompt = buildPrompt(event);
            io.stdout(`Launching one-shot pi session for ${event.type} on PR #${event.prNumber}...`);
            const exitCode = await runOneShot({ event, prompt, projectDir: args.projectDir, piBin: args.piBin });
            io.stdout(`One-shot pi session finished for PR #${event.prNumber} (exit ${exitCode}).`);
          } catch (error) {
            io.stderr(error instanceof Error ? error.message : String(error));
          } finally {
            runningPrs.delete(event.prNumber);
          }
        });

        await waitForShutdown(() => subscription.close());
        return 0;
      } catch (error) {
        io.stderr(error instanceof Error ? error.message : String(error));
        return 1;
      }
    }
  });

  const app = subcommands({
    name: "goddard-daemon",
    cmds: { run: runCmd }
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

function defaultRunOneShot(input: OneShotInput): number {
  const branchName = `pr-${input.event.prNumber}`;
  const agentsDir = `${input.projectDir}/.goddard-agents`;
  const worktreeDir = `${agentsDir}/${branchName}-${Date.now()}`;

  // Ensure agents dir exists
  spawnSync("mkdir", ["-p", agentsDir]);

  // Use copy-on-write clone to create the workspace instantly based on OS
  try {
    let cpArgs = ["-R", input.projectDir + "/", worktreeDir];
    if (process.platform === "darwin") {
      cpArgs = ["-cR", input.projectDir + "/", worktreeDir];
    } else if (process.platform === "linux") {
      cpArgs = ["--reflink=auto", "-R", input.projectDir + "/", worktreeDir];
    }

    let cloneResult = spawnSync("cp", cpArgs, { encoding: "utf8" });
    
    if (cloneResult.status !== 0 && process.platform === "darwin") {
      // Fallback to regular copy if APFS clone fails on macOS
      cloneResult = spawnSync("cp", ["-R", input.projectDir + "/", worktreeDir], { encoding: "utf8" });
    }

    if (cloneResult.status !== 0) {
      console.error(`\n[ERROR] Failed to create agent workspace at ${worktreeDir}`);
      console.error(`Attempted command: cp ${cpArgs.join(" ")}`);
      if (cloneResult.stderr) console.error(`Error output: ${cloneResult.stderr.trim()}`);
      if (cloneResult.error) console.error(`System error: ${cloneResult.error.message}`);
      console.error("Cannot proceed with one-shot pi session. Aborting.\n");
      return 1;
    }
  } catch (e) {
    console.error(`\n[ERROR] Exception thrown while creating agent workspace at ${worktreeDir}:`, e);
    return 1;
  }

  // Fetch and checkout the branch in the new workspace
  try {
    spawnSync("git", ["fetch", "origin", `pull/${input.event.prNumber}/head:${branchName}`], {
      cwd: worktreeDir,
      stdio: "ignore"
    });
    spawnSync("git", ["checkout", branchName], {
      cwd: worktreeDir,
      stdio: "ignore"
    });
  } catch(e) {
    // Ignore error
  }

  const sessionName = `pi-pr-${input.event.prNumber}-${Date.now()}`;
  const tmuxCmd = `tmux new-session -d -s ${sessionName} -c ${worktreeDir} "${input.piBin} '${input.prompt.replace(/'/g, "'\\''")}'"`;

  const result = spawnSync("sh", ["-c", tmuxCmd], {
    stdio: "inherit"
  });
  
  // Also log how to attach
  console.log(`\nStarted pi session in tmux. Attach with: tmux attach -t ${sessionName}\n`);

  return result.status ?? 1;
}

async function defaultWaitForShutdown(close: () => void): Promise<void> {
  await new Promise<void>((resolve) => {
    process.once("SIGINT", () => {
      close();
      resolve();
    });
  });
}

function isFeedbackEvent(event: RepoEvent): event is Extract<RepoEvent, { type: "comment" | "review" }> {
  return event.type === "comment" || event.type === "review";
}

function buildPrompt(event: Extract<RepoEvent, { type: "comment" | "review" }>): string {
  const feedback =
    event.type === "comment"
      ? `Comment from @${event.author}:\n${event.body}`
      : `Review from @${event.author} (${event.state}):\n${event.body}`;

  return [
    `You are responding to PR feedback for ${event.owner}/${event.repo}#${event.prNumber}.`,
    feedback,
    "Assess the feedback, apply any necessary repository changes, and finish by posting a reply on that PR thread explaining what you changed or why no change was needed.",
    "Do not switch to another PR; stay scoped to this event's PR."
  ].join("\n\n");
}

function splitRepo(repoRef: string): { owner: string; repo: string } {
  const [owner, repo] = repoRef.split("/");
  if (!owner || !repo) {
    throw new Error("repo must be in owner/repo format");
  }
  return { owner, repo };
}

const defaultIo: DaemonIo = {
  stdout: (line) => process.stdout.write(`${line}\n`),
  stderr: (line) => process.stderr.write(`${line}\n`)
};
