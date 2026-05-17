# SDK Agent Loop POC

This folder shows how an external Node/Bun script can use `@goddard-ai/sdk/node` to start a daemon-backed agent session with a system prompt read from disk.

Run it from the repository root while the Goddard daemon is available:

```sh
./sdk-agent-loop-poc/bin/sdk-agent-loop-poc \
  --system-prompt-file ./sdk-agent-loop-poc/system-prompt.md \
  --prompt "Say hello in one sentence." \
  --cycle-delay 5s \
  --max-iterations 3
```

Or run the interactive loop:

```sh
./sdk-agent-loop-poc/bin/sdk-agent-loop-poc \
  --system-prompt-file ./sdk-agent-loop-poc/system-prompt.md
```

Useful options:

- `--system-prompt-file, -s`: path to the system prompt file.
- `--cwd`: working directory for the agent session. Defaults to the current directory.
- `--agent`: optional ACP adapter name or distribution id. When omitted, the daemon resolves its default agent.
- `--model`: optional initial model id.
- `--prompt`: prompt to repeat each loop iteration. When omitted, the command asks interactively or reads piped stdin.
- `--cycle-delay`: delay between submitted prompts. Supports `ms`, `s`, `m`, `h`, and `d`; defaults to `0s`.
- `--max-iterations`: maximum number of prompts to submit before exiting.
- `--daemon-url`: optional daemon URL override. When omitted, the SDK resolves the daemon using its Node defaults.

The script intentionally denies permission requests so it is safe as a minimal proof of concept. Replace the `requestPermission` handler in `src/loop.ts` if your external host wants to approve tool calls.

When `--agent` or `--model` is omitted, the command prints the daemon-resolved default before the loop starts.

In an interactive terminal, press `Ctrl+C` to interrupt the current turn or delay. The loop asks for an optional custom prompt; press Enter with no text to resume the configured loop prompt.
