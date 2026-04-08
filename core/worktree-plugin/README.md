# @goddard-ai/worktree-plugin

Shared plugin types for Goddard worktree integrations.

This package defines the plugin contract used by the daemon worktree host and by third-party worktree plugins that need type alignment.

The daemon loads third-party worktree plugins from the global Goddard config via either:

- `{"type":"path","path":"plugins/my-plugin.mjs"}`
- `{"type":"package","package":"@acme/goddard-worktree-plugin"}`

Plugins should export a `WorktreePlugin` object, usually as the module's default export.
