# @goddard-ai/agent

The managed-agent feature owns launchable ACP agent discovery and runtime readiness.

This package replaces the old adapter-named boundary. ACP registry entries, config-declared
agents, Goddard-owned launch visibility markers, acp-client managed install state, and
managed-agent update policy are one product capability: deciding which agents can be
shown for launch and how a selected agent resolves to a runnable process.

## Boundary

Managed-agent owns:

- ACP registry reads and cache/fallback metadata used for agent discovery.
- Catalog merge behavior for registry and config-declared agents.
- Local launch visibility markers for catalog agents.
- Managed install status and process-spec resolution for acp-client managed agents.
- Managed-agent usage tracking and proactive update scheduling.
- Managed-agent daemon IPC, SDK namespace, schemas, tests, and docs.

Core daemon remains responsible for process lifecycle, plugin composition, root config
file substrate, logging, IPC, events, and persistence substrate.

Session consumes the managed-agent daemon extension for launch process resolution instead
of receiving registry or install services directly.
