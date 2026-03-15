## Goal
Define the data structures, behaviors, and limits of the autonomous execution loop to ensure predictable, constrained AI operations.

## Hypothesis
We believe that explicitly modeling the execution loop, configuration constraints, and rate limits will prevent runaway costs and provide stable unattended operations.

## Sub-Specs
- `spec/core/architecture.md`: System components and deployment topology.
- `spec/core/data-flows.md`: Conceptual end-to-end event sequences.

## Configuration Contract
- **Discovery**: Local workspace configs take precedence over global user configs.
- **Validation**: Configurations are strictly validated at startup. Invalid configs prevent the execution loop from starting.
- **Immutability**: Once the execution loop starts, the configuration cannot be modified.

## Rate Limiting & Constraints
Rate limiting constrains three dimensions of autonomous loop behavior:
1. **Cycle Cadence**: Minimum wall-clock delay between consecutive agent cycles.
2. **Operation Throughput**: Maximum allowed operations within a rolling time window.
3. **Token Consumption**: A hard cap on tokens consumed per individual cycle.

**Enforcement:**
- Token limits are evaluated after every cycle. If a cycle exceeds the per-cycle token budget, the loop immediately terminates.
- Throughput and cadence limits dictate the sleep duration before the next cycle begins.
- An optional pause threshold allows the loop to hibernate for an extended period after reaching a specific cycle count.

## Runtime Loop Behavior
- **State Machine**: `Initializing -> Throttling -> Prompting -> Evaluating -> (Continuing | Terminating)`
- **Persistence**: The loop utilizes a persistent agent session. Context from previous cycles is accumulated and carried forward.
- **Termination**: 
  - The loop terminates successfully if the agent explicitly signals completion.
  - The loop terminates with an error if hard limits (like token caps) are breached or if unrecoverable errors occur.
  - Recovery and restarts are the responsibility of the hosting runtime or external supervisor; the loop does not attempt complex internal self-healing for fatal errors.
