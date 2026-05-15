# SDK Agent Loop POC

This folder shows how an external Node/Bun script can use `@goddard-ai/sdk/node` to start a daemon-backed agent session with a system prompt read from disk.

Run it from the repository root while the Goddard daemon is available:

```sh
bun run ./sdk-agent-loop-poc/run-agent-loop.ts \
  --system-prompt-file ./sdk-agent-loop-poc/system-prompt.md \
  --prompt "Say hello in one sentence."
```

Or run the interactive loop:

```sh
bun run ./sdk-agent-loop-poc/run-agent-loop.ts \
  --system-prompt-file ./sdk-agent-loop-poc/system-prompt.md
```

Useful options:

- `--system-prompt-file, -s`: path to the system prompt file.
- `--cwd`: working directory for the agent session. Defaults to the current directory.
- `--agent`: optional ACP adapter name or distribution id. When omitted, the daemon resolves its default agent.
- `--model`: optional initial model id.
- `--prompt`: optional first user prompt before the interactive loop starts.
- `--daemon-url`: optional daemon URL override. When omitted, the SDK resolves the daemon using its Node defaults.

The script intentionally denies permission requests so it is safe as a minimal proof of concept. Replace the `requestPermission` handler in `run-agent-loop.ts` if your external host wants to approve tool calls.
