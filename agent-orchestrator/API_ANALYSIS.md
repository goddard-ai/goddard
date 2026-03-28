# Agent Orchestrator API Analysis

## API Documentation Sources

The REST API endpoints implemented in the provider classes are based on the conceptual example skeleton provided in the project specification. They represent the idealized, uniform interface required by the orchestrator.

Currently, these specific URLs act as placeholders or internal/mock targets based on the design spec:
- **Cursor Cloud**: `https://api.cursor.sh/agents`
- **Google Jules**: `https://api.jules.google.com/agents`
- **OpenAI Codex**: `https://api.openai.com/v1/codex/agents`

*Note: Since these were defined as conceptual skeletons in the "Ultra-Minimal Cloud Coding Agent Adapter" spec, there are no official public documentation links for these exact orchestration endpoints yet. When the official APIs for these agent-as-a-service endpoints become available, those documentation links should be added here.*

## Alternative Routes & Trade-offs

When building an orchestration adapter for cloud coding agents, there are a few alternative routes to interacting with the agents:

### 1. CLI Tools (e.g., executing `cursor-cli run ...`)
**Pros:**
- Often handles authentication, local repository context, and log streaming automatically.
**Cons:**
- **Environment Dependency:** Requires the CLI to be installed and available in the execution environment.
- **Parsing Overhead:** Orchestrators must parse `stdout`/`stderr` text streams instead of structured JSON, which is brittle and error-prone.
- **Inconsistent Interfaces:** Each provider has entirely different CLI flags, output formats, and exit codes.

### 2. Provider-Specific SDKs (e.g., `npm install @cursor/sdk`)
**Pros:**
- Strongly typed, officially supported, and handles retries/polling internally.
**Cons:**
- **Bloat:** Increases bundle size and pulls in transitive dependencies.
- **Overfitting:** SDKs often expose complex abstractions (streaming logs, environment setups) that violate our "Ultra-Minimal" core design principle.

### 3. Direct HTTP REST API Polling (Current Route)
**Why it is the best:**
- **Zero Dependencies:** Relies solely on native `fetch`. It requires no third-party SDKs, CLI tools, or heavy packages.
- **Universal Abstraction:** Every provider conceptually behaves like a job system (`submit -> wait -> get result`). Using HTTP APIs directly allows us to map everything directly to the universal `AgentJob` interface without fighting provider-specific abstractions.
- **Extremely Stable:** HTTP endpoints for job status and retrieval rarely change their fundamental shape, making this the most robust and maintainable route.
- **Hides Provider Weirdness:** The adapter layer handles the network boundary, ensuring the orchestration app never knows about provider-specific CLI usage, SDK nuances, or authentication quirks.
