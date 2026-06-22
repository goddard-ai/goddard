# Managed Agents

Managed agents are ACP agents that can be shown or selected when starting daemon-managed sessions. This page explains how the daemon presents catalog entries, local launch visibility, managed install status, and runnable process resolution.

## Core idea

- Managed agents describe launchable ACP agents for daemon-managed sessions.
- The managed-agent catalog answers which agents can be shown or selected in a global or project launch context.

## Catalog sources

- Registry-provided ACP agent entries.
- User or repository configuration entries.
- Configuration-declared entries are launch-visible because the user or repository made them part of effective configuration.

## Local install state

- Goddard-owned launch visibility state records which registry agents the user enabled for normal launch listings.
- Ordinary registry agents can remain hidden from launch listings until enabled.
- Clients may request broader listings when presenting catalog management.
- Launch listings should make the difference visible between enabled, merely available, configuration-declared, and managed-install entries.

## Install and uninstall

- Installing a catalog managed agent changes local launch catalog visibility.
- Uninstalling it removes that local launch catalog marker.
- Neither action starts an agent session by itself.
- If an installed entry later cannot resolve a runnable process, launch should fail as a launch problem rather than treating the catalog marker as proof of runtime readiness.

## Boundaries

- Listing a managed agent does not guarantee launch until the managed-agent feature can resolve a runnable process.
- Local launch visibility state is separate from acp-client managed install state.
- Repository-local catalog entries should not silently grant executable extension authority beyond the configured agent behavior.
