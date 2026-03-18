# Workforce Orchestration Package

This package provides a Command Line Interface (CLI) for managing "workforce sessions" within a Goddard-compatible repository. It facilitates a multi-agent orchestration pattern where specific directories (packages) in a codebase are assigned dedicated autonomous AI agents. These agents communicate asynchronously via "inbox" files located in a `.goddard/` directory within each package.

The system is designed for **Workforce Lead Agents** (per-package) and a **Workforce Root Agent** (repository-wide) to collaborate on complex tasks by appending requests and responses to JSONL files.

---

## High-Level Architecture

### The CLI Layer
The CLI (`goddard-workforce`) is built using `cmd-ts` and `@clack/prompts`. It provides two primary commands:

*   **`init`**: 
    *   Scans the repository for packages (directories containing `package.json`).
    *   Prompts the user to select packages for workforce enablement.
    *   Creates a `.goddard/` directory in each selected package containing empty `requests.jsonl` and `responses.jsonl` files.
*   **`watch`**:
    *   Resolves the repository root.
    *   Discovers all packages that have a `.goddard/` directory.
    *   Spawns a persistent `WorkforceSupervisor` (from `@goddard-ai/sdk`) to manage the lifecycle of AI agents for those packages.

---

## Technical Orchestration (SDK Logic)

The core logic resides in `@goddard-ai/sdk/node/workforce.ts`, which manages state, file watching, and agent communication.

### Component Breakdown

#### WorkforceSupervisor & Package Runtime
*   **`WorkforceSupervisor`**: Manages a collection of `WorkforcePackageRuntime` instances.
*   **`WorkforcePackageRuntime`**: Encapsulates the state for a single package, including:
    *   An `AgentSession` connected to the Goddard daemon.
    *   A `chokidar` watcher monitoring the `.goddard/` directory.
    *   A prompt queue for handling concurrent file updates.

#### Inbox Protocol
Each workforce-enabled package contains:
*   `requests.jsonl`: Inputs for the agent.
*   `responses.jsonl`: Outputs/logs from the agent.
*   `processed-at.json`: Tracks the byte offset and SHA-256 hash of the last processed line in the JSONL files.

### Workflow & Logic Flow

1.  **Discovery**: Uses `git rev-parse --show-toplevel` and recursive directory walking to find packages containing a `.goddard` directory.
2.  **Session Initialization**: For each package, it starts a persistent `pi` agent session via the Goddard daemon. 
    *   **Root Agent**: Assigned to the repository root. Instructed to own the project-wide view and route requests.
    *   **Lead Agent**: Assigned to sub-packages. Instructed to own only the code inside their specific directory.
3.  **File Watching**: Monitors the `.goddard/` directory for appends.
4.  **Append Tracking**:
    *   Reads `processed-at.json` to find the previous `offset`.
    *   Verifies the `prefixHash` to ensure file integrity. If the prefix hash doesn't match (e.g., the file was rewritten), it resets tracking.
    *   Extracts only the newly appended lines.
5.  **Prompting**: Formats the new JSONL records into a prompt and sends it to the agent session. If the agent is already busy, batches are queued and processed sequentially.

---

## Implementation Details

### Data Models

#### Processed State (`WorkforceProcessedState`)
Stored in `.goddard/processed-at.json`.
```json
{
  "version": 1,
  "files": {
    "requests.jsonl": {
      "offset": 1234,
      "prefixHash": "sha256...",
      "updatedAt": "ISO-TIMESTAMP"
    }
  }
}
```

### Prompt Construction
When activity is detected, the agent receives a prompt structured with:
1.  **Header**: Contextualizing the package name and relative path.
2.  **Instruction**: Explicit directives to process the newly appended JSONL records.
3.  **Batches**: Multiple batches of JSONL records wrapped in ```jsonl``` blocks.

---

## Critical Execution Logic

### Robustness
The system uses `Buffer.byteLength` and `findLastCompleteOffset` to ensure it only reads full lines from the JSONL files, maintaining integrity across multi-byte UTF-8 characters.

### Recursive Synchronization
The `drainPackageQueue` function ensures that if new requests arrive while the agent is still processing a previous batch, they are queued and sent immediately after the current task finishes, preventing loss of intent during high-activity periods.

### Daemon Dependency
The workforce CLI requires a running Goddard daemon to manage the underlying AI sessions.

## License

This project is licensed under the [MIT License](./LICENSE).
