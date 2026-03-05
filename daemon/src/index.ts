import { createSdk, type RepoEvent } from "@goddard-ai/sdk";
import { spawnSync } from "node:child_process";
import * as pty from "node-pty";
import { command, option, runSafely, string, subcommands } from "cmd-ts";
import { FileTokenStorage } from "./storage.ts";
import { LocalSessionStorage } from "@goddard-ai/storage";

export type DaemonIo = {
  stdout: (line: string) => void;
  stderr: (line: string) => void;
};

type SdkClient = ReturnType<typeof createSdk>;

type OneShotInput = {
  event: Extract<RepoEvent, { type: "comment" | "review" | "proposal.merged" }>;
  prompt: string;
  projectDir: string;
  piBin: string;
  sessionName: string;
  ptyProcessCb?: (ptyProcess: pty.IPty) => void;
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
        const runningPrs = new Map<number, { sessionName: string; isTmux: boolean; ptyProcess?: pty.IPty }>();
        const subscription = await sdk.stream.subscribeToRepo({ owner, repo });

        io.stdout(`Daemon subscribed to ${owner}/${repo}. Waiting for PR feedback events...`);

        subscription.on("event", async (payload) => {
          const event = payload as RepoEvent;
          if (!isTriggerEvent(event)) {
            return;
          }

          const prompt = buildPrompt(event);
          const activeSession = runningPrs.get(event.prNumber);

          if (activeSession) {
            if (event.type === "proposal.merged") {
              io.stdout(`Warning: A session is already active for PR #${event.prNumber}. Cannot inject a proposal merge event into an existing session.`);
              return;
            }

            io.stdout(`Injecting new feedback into existing session for PR #${event.prNumber}.`);
            try {
              if (activeSession.isTmux) {
                // Send the new prompt as input to the running tmux session via send-keys
                spawnSync("tmux", [
                  "send-keys",
                  "-t",
                  activeSession.sessionName,
                  `"${prompt.replace(/"/g, '\\"')}"`,
                  "Enter"
                ]);
              } else if (activeSession.ptyProcess) {
                activeSession.ptyProcess.write(`\n${prompt}\n`);
              } else {
                io.stderr(`Cannot inject feedback dynamically. Ignoring feedback.`);
              }
            } catch (err) {
              io.stderr(`Failed to inject feedback into session for PR #${event.prNumber}: ${err}`);
            }
            return;
          }

          const sessionName = `pi-pr-${event.prNumber}-${Date.now()}`;
          const isTmux = spawnSync("which", ["tmux"]).status === 0;
          runningPrs.set(event.prNumber, { sessionName, isTmux });

          try {
            const managed = await sdk.pr.isManaged({ owner: event.owner, repo: event.repo, prNumber: event.prNumber });
            if (!managed) {
              io.stdout(`Ignoring ${event.type} on unmanaged PR #${event.prNumber}.`);
              return;
            }

            io.stdout(`Launching one-shot pi session for ${event.type} on PR #${event.prNumber}...`);

            let sessionId: number | undefined;
            let sessionStorage: LocalSessionStorage | undefined;
            try {
              sessionStorage = new LocalSessionStorage();
              const sessionRecord = await sessionStorage.createSession(
                event.owner,
                event.repo,
                event.prNumber
              );
              sessionId = sessionRecord.id;
            } catch (err) {
              io.stderr(`Failed to record active pi session locally: ${err}`);
            }

            const exitCode = await runOneShot({ 
              event, 
              prompt, 
              projectDir: args.projectDir, 
              piBin: args.piBin, 
              sessionName,
              ptyProcessCb: (ptyProcess) => {
                const session = runningPrs.get(event.prNumber);
                if (session) session.ptyProcess = ptyProcess;
              }
            });

            if (sessionId !== undefined && sessionStorage) {
              try {
                await sessionStorage.updateSession(sessionId, "completed");
              } catch (err) {
                io.stderr(`Failed to mark active pi session as completed locally: ${err}`);
              }
            }

            io.stdout(`One-shot pi session finished for PR #${event.prNumber} (exit ${exitCode}).`);
          } catch (error) {
            io.stderr(error instanceof Error ? error.message : String(error));
          } finally {
            if (runningPrs.get(event.prNumber)?.sessionName === sessionName) {
              runningPrs.delete(event.prNumber);
            }
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

async function defaultRunOneShot(input: OneShotInput): Promise<number> {
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
    let fallbackAttempted = false;
    
    if (cloneResult.status !== 0 && process.platform === "darwin") {
      // Fallback to regular copy if APFS clone fails on macOS
      fallbackAttempted = true;
      cpArgs = ["-R", input.projectDir + "/", worktreeDir];
      cloneResult = spawnSync("cp", cpArgs, { encoding: "utf8" });
    }

    if (cloneResult.status !== 0) {
      console.error(`\n[ERROR] Failed to create agent workspace at ${worktreeDir}`);
      if (fallbackAttempted) {
        console.error("Attempted APFS clone (cp -cR) and fallback copy (cp -R). Both failed.");
      }
      console.error(`Last attempted command: cp ${cpArgs.join(" ")}`);
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

  // Check if tmux is installed
  const hasTmux = spawnSync("which", ["tmux"]).status === 0;

  let result;
  if (hasTmux) {
    const tmuxCmd = `tmux new-session -d -s ${input.sessionName} -c ${worktreeDir} "${input.piBin} '${input.prompt.replace(/'/g, "'\\''")}'"`;

    result = spawnSync("sh", ["-c", tmuxCmd], {
      stdio: "inherit"
    });
    
    // Also log how to attach
    console.log(`\nStarted pi session in tmux. Attach with: tmux attach -t ${input.sessionName}\n`);
    return result.status ?? 1;
  } else {
    // Fallback: use node-pty to spawn the process so we can inject stdin
    return new Promise<number>((resolve) => {
      const ptyProcess = pty.spawn(input.piBin, [input.prompt], {
        name: 'xterm-color',
        cols: 80,
        rows: 30,
        cwd: worktreeDir,
        env: process.env as any
      });

      if (input.ptyProcessCb) {
        input.ptyProcessCb(ptyProcess);
      }

      ptyProcess.onData((data) => {
        process.stdout.write(data);
      });

      ptyProcess.onExit(({ exitCode }) => {
        resolve(exitCode);
      });
    });
  }
}

async function defaultWaitForShutdown(close: () => void): Promise<void> {
  await new Promise<void>((resolve) => {
    process.once("SIGINT", () => {
      close();
      resolve();
    });
  });
}

function isTriggerEvent(event: RepoEvent): event is Extract<RepoEvent, { type: "comment" | "review" | "proposal.merged" }> {
  return event.type === "comment" || event.type === "review" || event.type === "proposal.merged";
}

function buildPrompt(event: Extract<RepoEvent, { type: "comment" | "review" | "proposal.merged" }>): string {
  if (event.type === "proposal.merged") {
    return [
      `You are responding to a merged roadmap proposal for PR #${event.prNumber}.`,
      `Implement the changes described in the proposal.`,
      "Do not switch to another PR; stay scoped to this event's PR."
    ].join("\n\n");
  }

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
