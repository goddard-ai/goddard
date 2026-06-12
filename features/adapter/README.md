# @goddard-ai/adapter

The adapter feature exposes the ACP adapter catalog to daemon, SDK, and app callers.

## Catalog Sources

Adapter listing merges two sources:

- acp-client registry data from the daemon registry service.
- project or user `registry` config entries, which override registry entries with the same id.

Config-declared registry entries are always launch-visible because the user already made them part of the effective project configuration.

## Local Adapter Installs

`adapter-installations.json` is Goddard-owned launch catalog state. It records which registry adapters the user has enabled for normal launch listings. Ordinary registry adapters are hidden from launch listings until they are locally installed, unless the caller asks for `includeUninstalled`.

This state is separate from acp-client managed agent installs.

## Managed Agent Installs

`agents.managed` is daemon policy for acp-client-owned runtime install and update state. Managed agents can be installed before use or updated proactively by the daemon install service after use in the last 30 days, while acp-client owns the persisted install metadata and runnable process-spec resolution.

A managed agent can appear in launch listings even when it has no Goddard local adapter-install marker. In that case the adapter entry includes `managedInstall` status from the daemon install service.
