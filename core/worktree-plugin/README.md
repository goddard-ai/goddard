# @goddard-ai/worktree-plugin

Shared plugin types for Goddard linked-worktree integrations.

This package defines the plugin contract used by the daemon worktree host and by third-party worktree plugins that need type alignment.

Plugins are expected to provision real linked Git worktrees attached to the source repository. Returning arbitrary directories or independent repositories is not supported by the daemon worktree host.

The daemon loads third-party worktree plugins from the global Goddard config via either:

- `{"type":"path","path":"plugins/my-plugin.mjs"}`
- `{"type":"package","package":"@acme/goddard-worktree-plugin"}`

Plugins should export a `WorktreePlugin` object, usually as the module's default export.
