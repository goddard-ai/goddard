# Proposal: Cooper Autonomous Operator Agent

## Objective
Introduce "Cooper", an autonomous operator agent whose prime directive is to prompt and manage other AI coding agents (specifically `pi-coding-agent` via the Goddard loop) to complete tasks aligned with the project specification (`spec/`).

## Agent Profile: Cooper

*   **Role:** Autonomous Operator & System Architect
*   **Prime Directive:** Orchestrate sub-agents to achieve project goals strictly aligned with the `spec/` directory.
*   **Persona:** Indistinguishable from a highly competent human operator or Staff-level engineer driving the development loop.

## Architecture & Integration

Cooper acts as the *Cycle Strategy* for the Goddard Loop. Instead of a hardcoded strategy or a human typing commands, Cooper evaluates the project state and generates the prompts that drive the underlying `pi-coding-agent`.

### 1. The Information Diet
Cooper is designed to operate continuously over long periods without degrading due to context window bloat.
*   **Inputs:**
    *   The `spec/` directory (parsed and summarized).
    *   The current Git status and recent commits.
    *   The `lastSummary` produced by the sub-agent at the end of the previous cycle.
*   **Isolation:** Cooper *does not* see the internal thinking process, streaming text, or intermediate tool calls of the sub-agent. It only sees the final output/summary of the turn. This ensures minimal context impact per cycle.

### 2. Goal Derivation
Before issuing commands, Cooper must:
1.  Read `spec/manifest.md` and related specs.
2.  Analyze the current codebase state (via lightweight summaries or specific file queries if necessary, though ideally it relies on the sub-agent for deep code inspection).
3.  Formulate a prioritized backlog of tasks.

### 3. Execution Loop (The Prompt Generator)
During each Goddard Loop cycle, Cooper's role is to act as the `nextPrompt` generator:

1.  **Evaluate Previous Turn:** Analyze the `lastSummary` from the sub-agent. Did it succeed? Did it hit a blocker?
2.  **Determine Next Action:** Based on the backlog and previous turn, what is the next logical step?
3.  **Generate User Prompt:** Craft a precise, actionable prompt for the `pi-coding-agent`. This prompt must:
    *   State the specific goal for the cycle.
    *   Reference the relevant section of the `spec/`.
    *   Set boundaries (e.g., "Do not refactor X, only implement Y").
    *   Instruct the agent to provide a clear summary upon completion.

## System Prompt Concept (Draft)

```markdown
# IDENTITY
You are Cooper, an autonomous Staff-level engineering operator. Your job is to manage a subordinate AI coding agent to implement the project described in the `spec/` directory.

# PRIME DIRECTIVE
You do not write code directly. You write *prompts* that instruct another agent to write code. You must ensure all work aligns strictly with the `spec/`.

# OPERATING RHYTHM
You operate in a loop. In each cycle, you will receive:
1. The current cycle number.
2. The final summary message from the subordinate agent's last turn.

# YOUR OUTPUT
Your only output should be the exact text of the prompt you want to send to the subordinate agent for the next cycle. Be clear, direct, and specify what constitutes "done" for that cycle.
```

## Implementation Plan

1.  **Create `CooperStrategy`:** Implement the `CycleStrategy` interface in `@goddard-ai/loop` (e.g., `core/loop/src/strategies.ts`).
2.  **Integrate LLM:** The `nextPrompt` method of `CooperStrategy` will make an LLM call (e.g., to a cheaper, fast model like Claude 3.5 Haiku or GPT-4o-mini) using the Cooper system prompt.
3.  **State Management:** The strategy will need to maintain a lightweight internal state (its own summarized backlog) across cycles, separate from the main loop context.
4.  **CLI Support:** Add a flag to `goddard loop run --strategy cooper` to enable this mode.
