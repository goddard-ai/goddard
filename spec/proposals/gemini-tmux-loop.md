# Proposal: Gemini CLI and Tmux Support for Goddard Loop

## Objective
Enable running the Goddard loop using Gemini CLI inside a Tmux session, providing an alternative to the standard `pi-coding-agent` while retaining the core rate limiting and strategy logic of the loop.

## Background
Currently, the Goddard loop uses `@mariozechner/pi-coding-agent` as the execution engine for AI operations. Users might want to drive the loop using the Gemini CLI (`gemini`) running in a Tmux session for better interactive observation, custom environments, or utilizing Gemini-specific features.

The `GEMINI_SYSTEM_MD` environment variable allows overriding the normal Gemini CLI system prompt, which is crucial for passing the loop strategy and context to the model.

## Required Changes

### 1. Configuration Changes (`core/config/src/index.ts`)
We need to update the configuration schema to support an alternative execution engine or generic command executor.

*   Add a new configuration type (e.g., `TmuxCommandConfig`) to `PiAgentConfig` or as a top-level alternative to `agent`.
*   Fields required:
    *   `command`: The base command to run (e.g., `"gemini"`).
    *   `tmuxSessionName`: The name of the tmux session to create/attach to.
    *   `args`: Additional arguments for the command.
*   *Alternatively*, if we want to keep it generic, allow specifying a custom executor function or command template. However, given the specific request for Gemini and Tmux, direct support or a well-documented generic wrapper is needed.

### 2. Loop Runtime Engine (`core/loop/src/index.ts`)
The `createLoop` function currently hardcodes the creation of an `AgentSession` and `InteractiveMode` from `@mariozechner/pi-coding-agent`.

*   **Refactor `endlessLoop`:**
    *   Abstract the session creation and execution logic.
    *   If the configuration specifies a Tmux/Gemini executor, bypass `createAgentSession` and `InteractiveMode`.
    *   Instead, spawn a `tmux` process.
    *   Command template: `tmux new-session -d -s <session-name> "<command>"` or send keys to an existing session.
*   **Prompt Injection:**
    *   The loop strategy generates a `prompt` for each cycle.
    *   For the Gemini CLI, this prompt needs to be passed effectively.
    *   The proposal notes that `GEMINI_SYSTEM_MD` overrides the system prompt. We can write the generated prompt to a temporary file and set `GEMINI_SYSTEM_MD=/path/to/temp/file.md` before invoking the Gemini CLI inside Tmux.
    *   *Self-Correction*: The loop sends a user prompt every cycle, not just a system prompt. If the Gemini CLI is interactive, we need a way to send the cycle prompt to the running tmux session (e.g., `tmux send-keys -t <session> "prompt text" Enter`). If it's one-shot per cycle, setting `GEMINI_SYSTEM_MD` and running `gemini <prompt>` inside tmux is the approach. Assuming one-shot per cycle for simplicity, or we need to manage the interactive stdin.
*   **Token Counting & Done Signals:**
    *   `pi-coding-agent` provides token usage via `session.getSessionStats()`. We need a mechanism to extract this from the Gemini CLI output or estimate it if driving via Tmux.
    *   The loop relies on `isDoneSignal(lastSummary)`. We must capture the output of the Gemini CLI from the Tmux session (e.g., using `tmux capture-pane -t <session> -p`) to check for the "DONE" signal.

### 3. Implementation Details for Tmux integration
*   **Starting the session:**
    ```bash
    # Prepare prompt
    echo "cycle prompt..." > /tmp/goddard-cycle-prompt.md

    # Run in tmux
    tmux new-session -d -s goddard-loop "GEMINI_SYSTEM_MD=/tmp/goddard-cycle-prompt.md gemini ..."
    ```
*   **Waiting for completion:** The loop needs to wait for the command inside Tmux to finish before starting the next cycle. This might involve polling `tmux list-sessions` or using a wrapper script that signals completion.
*   **Capturing Output:** After completion (or periodically), use `tmux capture-pane` to get the assistant's response to check for the "DONE" signal.

### 4. SDK Updates (`sdk/src/index.ts`)
No major changes expected in the SDK itself unless we expose specific CLI commands to manage these Tmux sessions, but the core loop runtime handles the execution.

### Summary of Action Items
1.  Update `core/config/src/index.ts` to type a `tmuxExecutor` config block.
2.  Modify `core/loop/src/index.ts` to branch execution logic based on whether a `piAgent` or `tmuxExecutor` is configured.
3.  Implement a `TmuxSessionRunner` class that handles:
    *   Writing the loop strategy prompt to a temporary file.
    *   Executing `tmux new-session ... env GEMINI_SYSTEM_MD=... gemini`.
    *   Waiting for pane completion.
    *   Capturing output via `tmux capture-pane` to parse the `lastSummary` and check for `DONE`.
