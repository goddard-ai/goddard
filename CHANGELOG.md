# Changelog

All notable changes to Goddard. This file is the **source of truth for the release
notes shown in the in-app updater**: [`scripts/release.ts`](scripts/release.ts)
extracts the section whose heading matches the version being released
(`MARKETING_VERSION`) and publishes it next to the update, so Sparkle shows it in
the update prompt.

Format follows [Keep a Changelog](https://keepachangelog.com). Don't edit this
file directly for pending changes — add a `.changelog/<prefix>-<slug>.md`
fragment per change (one bullet per file) so parallel work never conflicts
here. The required prefix picks the release-notes section: `highlight-` for
headline features, `feat-` for other features, `exp-` for experimental
opt-ins (emitted under `### Experiments` with a bold `[Experimental]` marker;
experiments are never highlights), `fix-` for bugs that existed in a released
version. Every `highlight-` fragment must embed a screenshot or
recording — `![](media/<slug>.{png,gif,mp4,mov})` with the asset at
`.changelog/media/<slug>.<ext>` — which collect moves to
`assets/release-notes/<version>/` and rewrites in the emitted section. At
release time `bun run changelog` folds every fragment into a `## [<version>]`
section for the version in `Cargo.toml`, grouped under `### Highlights`,
`### Features`, `### Experiments`, and `### Fixed`.

Write release notes for the final product users receive, not the development
history. When a feature is still unreleased, fold its fixes and refinements into
the original feature bullet instead of adding separate entries for them.

## [0.2.1]

### Features

- Archiving a task whose turn is still running now asks for confirmation first instead of silently stopping the turn
- Jump to file:line[:column] straight from the ⌘P finder
- Rebind ⌘T to always open a new terminal rooted in the current context — the on-screen terminal's directory, the selected session's workspace, or ~ — moving the Terminals chord to ⌘⇧T and leaving Toggle Workspace reachable from the menu, command palette, and Work-in chip; ⌘⇧N now toggles the workspace on the new task page
- Add a real compact command: the daemon routes compaction through the provider instead of faking it with a prompt, and Codex and OpenCode 2 show context compaction as its own activity card
- Preview images and SVGs in the file viewer — refreshed from just-saved bytes — and jump to a line with ctrl-g
- Add a Settings → Friends page for friend-to-friend file transfers: probe-based presence, incoming requests you accept or deny, live byte progress with a sidebar footer ring, and received sessions materialized under a synthetic Friends project behind an explicit trust quarantine
- Add GitHub Copilot as a provider via the official Rust SDK, with rewind and branch through `sessions.fork` and composer attachments sent as structured attachments
- Rename the product to Goddard and migrate legacy Waku state
- New "High contrast borders" setting (Appearance) widens border-tier contrast so control outlines meet WCAG's 3:1 non-text floor; it turns on automatically when the OS Increase Contrast accessibility setting is enabled
- Borders, separators, and outlines are softer by default — hairline rules sit at 1.4:1 and filled surfaces like the composer, cards, menus, and tooltips use a new subtler outline tier
- Add a keybinding manager page — a searchable command catalog over a keyboard stage — with a capture editor for live rebinding, conflict surfacing gated behind an explicit confirm, a manual keyboard-layout picker, and overrides persisted to keybindings.json and applied at startup
- Open @-mention file references from user prompts in the right panel
- Retarget ⌘D as "Go to next unread completion": it always picks the topmost unread row, cycles idle sessions once the unread queue drains, skips sessions with queued prompts, and ⌘⇧D is now a positional sweep instead of a next-unread jump
- The Projects page's docked composer gains the chat footer's project, worktree, and branch pickers and a working model selector, so a task is fully configured before it is created
- Connect to remote hosts over SSH — managed from daemon settings, badged in the sidebar, with masked-password askpass prompts and port forwarding — so their sessions, skills, and usage appear alongside local tasks, and the daemon installs and upgrades itself on the host
- Add a sandbox environment choice to the composer's access menu
- Move worktree and branch settings into a dedicated Settings → Git page
- Cap each sidebar project group at sixteen chats; a "Show more" row reveals the rest in batches instead of folding only chats older than three days
- Sidebar terminal rows no longer show a checkmark when a command exits cleanly; a terminal that finishes while off-screen now gets an unread dot that clears when the terminal is next focused
- New opt-in setting fast-forwards the local default branch — plus any extra branches you list — to its tracking branch before a new worktree is based on it

### Experiments

- **[Experimental]** The Projects page (⌘⇧P) — a project's worktrees, branches, issues, and pull requests in one place — moves behind an Experiments opt-in and is now off by default

### Fixed

- Edit the annotation under an ⌥-click instead of stacking a new one on top
- Computer Use no longer fails with "socket path is too long": its bridge socket now lives in a private directory under `/tmp`, short enough for macOS's 104-byte `sun_path` limit regardless of `TMPDIR`
- Diff highlights and the scrollbar in the file-diff hover cards no longer paint past the card's rounded bottom corners
- Detect rebase state directories in the git panel instead of relying on a stale REBASE_HEAD
- `/land` shows a spinner toast while it runs and resolves it to the outcome, instead of giving no feedback until the land finished
- Round markdown table header corners to match the frame
- Render backticks glued to shortcut keycaps as literal keycaps instead of breaking inline code
- Git panel confirmation modals, agent permission prompts, and computer-use approvals are now keyboard-operable — Tab moves between options, Enter or Space activates the focused one, Enter on a modal card runs its primary action, and Escape cancels or denies
- Restore the ⌘N hint on New Task surfaces
- "Resolve in chat" in the git panel conflict modal now pastes the resolution prompt into the composer instead of sending it, so you can review or edit it before sending
- Drop the unbound Toggle Workspace row from the shortcuts dialog
- Use the session's display title when dragging it in the sidebar
- Paint the sidebar solid when macOS Reduce Transparency is on, and default sidebar transparency off on setups where no backdrop blur exists
- Keep worktree badges, branch labels, and checkout status on sidebar tasks across restarts instead of waiting for each task to be resumed

## [0.2.0]

### Features

- Annotate transcript lines and file-editor selections with comments that fold into the next prompt (⌥-click a line, ⌘L on a selection); "Annotation N" references resolve to hover citations
- Highlight and auto-continue Markdown lists in the composer, collapse large pastes into expandable cards, and accept file drops anywhere in the session
- Continue an interrupted session by typing in its empty composer
- Add user-defined custom commands to the command palette — with custom icons and toast notifications — and let agents manage them through daemon settings
- Add an Experiments settings page where unfinished features — Big Picture, the Git panel, GitHub integration, and subagents — can be turned on individually; all default off
- Open workspace files in the right panel with a ⌘P file finder and preview Markdown files fullscreen
- Add UI and code font family pickers and a separate terminal font size to Appearance settings; right-panel cards adapt cleanly when the UI font size is increased
- Hide the app with Cmd+H on macOS
- Play a sound when a background task finishes its turn, with a volume slider and in-selector previews, and show an unseen-completion bell in the top bar
- Add a Projects page listing worktrees, branches, issues, and pull requests
- Add Devin and Droid (Factory) as agent providers
- Recover renamed or moved project folders instead of losing them, and archive projectless workspaces automatically
- Run project scripts from a ⌘R picker and generate terminal commands with the session's agent
- New settings: thick borders, sidebar transparency, a Markdown preview toggle, and an opt-in response token speed readout
- Surface keyboard shortcuts in menus, tooltips, the command palette, a hold-⌘ overlay, and a cheatsheet next to the sidebar settings icon
- Peek at the closed sidebar by hovering the left window edge
- Pin, archive, and mark sessions unread from the sidebar, drag sessions into the composer as reference chips, and jump between tasks with ⌘1–9, ⌘D, and ⌘⇧D
- Syntax-highlight Lua, PHP, Zig, Dart, Elixir, and Astro code blocks
- Reshape the ⌃⇥ task switcher into a compact recently-viewed list with session status glyphs
- Add a Terminals group to the sidebar showing live command status, working directory, and last-activity time; ⌘J focuses the session terminal
- Add 15 new themes (Dracula, Rosé Pine, Kansō, Gruvbox, GitHub, and more), split the theme preference into separate light and dark slots with a system-following toggle, and preview themes while browsing the selectors
- Create named adjective-noun worktrees beside the repository, move a session into a new worktree, and remove an archived task's worktree behind a snapshot ref

### Experiments

- **[Experimental]** Add Big Picture mode on ⌘0 — a full-window grid of session cards with live transcripts, per-card composers, and ⌘1–9 arming
- **[Experimental]** Add a Git panel (⌘⌥G) with a commit graph, expandable diffs, upstream tracking, and a /land command to land a worktree on its base
- **[Experimental]** Browse a project's GitHub issues and pull requests, start tasks from them, and track check and review status on sidebar rows and in the right panel
- **[Experimental]** Delegate runs to named subagents with per-provider tiers and cost-labeled routing, shown legibly in the transcript

### Fixed

- Keep Amp threads resumable after restarting the app
- Fix escaped backticks rendering literally inside inline code
- Prefer exact matches in the branch selector's filter
- Fix Cursor model discovery and model options
- Fix the IME candidate popup appearing in the wrong position
- Fix Waku→Goddard migration staging a full copy of `~/.waku` on every launch: workspaces, worktrees, and archives are now linked item-by-item instead of copied, the state database is cloned only when its schema is known, and abandoned `.migrating-*` staging directories are swept on launch
- Apply OpenCode model and effort changes to the live session instead of the next one
- Stop rendering blank reasoning-only lines in the transcript
- Fix remote images failing to load in transcripts
- Fix the transcript segment left behind when steering an in-flight reply
- Stop a quick ⌃⇥ chord from flashing the task switcher
- Stop warning about unpushed commits that are already merged into a branch
- Fix interrupted session saves erasing stored workspace details
- Fix startup on Macs without Xcode installed

## [0.1.19]

- Render inline and block LaTeX math in Markdown, with a Copy Expression action and a setting to show the source
- Fix OpenCode 2 context usage to reflect the latest request instead of cumulative session totals, and cap the meter at 100%
- Clear model search when selecting a provider or Favorites tab
- Make loading spinners smoother with animations at up to 60 FPS

## [0.1.18]

- Fix Codex session forking
- Add OpenCode 2 support, add reasoning effort option for both OpenCode and OpenCode 2
- Fix memory usage for long-running sessions

## [0.1.17]

- Fix the OpenCode Resume list showing only sessions started outside a git checkout; it now lists sessions from every project
- Hold Claude's turn open while it waits on background work

## [0.1.16]

- Import and continue conversations started in agent CLIs with `/resume` or the command palette across every provider, in both Goddard and Goddard Web
- Linux: add signed in-app updates with clean relaunch and automatic rollback
- Copy Goddard task IDs and agent CLI thread IDs from task info or the command palette
- Keep each response's actions and changed-file summary after its final tool activity
- Keep the selected task visible when navigating the sidebar
- Let nested transcript and command-output scrollers hand wheel gestures to the page only at their boundaries
- Keep multiline background-work titles on one line in summaries and panel headers

## [0.1.15]

- Codex thread goals: type /goal to set a persistent objective the task keeps pursuing — before or after the first message — with its autonomous pursuit streaming into the transcript, a status chip showing live budget or elapsed time, and a dialog to edit, pause, resume, or clear the goal (also in Goddard Web)
- Discover provider-native slash commands and skills from installed agent CLIs, including multiline YAML descriptions
- Add reasoning effort selection for Grok
- Reconnect remote daemon sessions automatically after connection interruptions
- Fix Command/Ctrl+Enter steering after a provider response starts streaming
- Fix transcript file links on Windows
- Fix OpenCode dropping the first streamed event and hanging during cancellation on Windows

## [0.1.14]

- Group sidebar tasks by project or update date, order them newest or oldest first, and collapse sections
- Find in page: Search the full transcript by keywords using cmd-f or ctrl-f
- Switch between recent tasks with Ctrl+Tab and Ctrl+Shift+Tab
- Carry the current access mode into new tasks and remember it between launches
- Fix OpenCode access-mode permissions and restore pending permission prompts when resuming sessions
- Show Codex file reads, listings, and searches as file activity instead of raw commands
- Keep long panel and background-work titles on one truncated line
- Increase the minimum UI text size for better legibility

## [0.1.13]

- Add Vercel Fx support
- Support DeepSeek Harness 0.1.1 without opening its web UI
- Collapse earlier activity groups when a running turn moves on to newer transcript output

## [0.1.12]

- Invoke Codex, Pi, and Oh My Pi skills with their native syntax
- Stream live output from Claude background tasks
- Steer the oldest queued follow-up with Command/Ctrl+Enter in an empty composer
- Fix model and reasoning option selection for Cursor
- Fix npm-installed provider detection on Windows
- Fix daemon terminal sessions hanging during shutdown
- Exclude copied history from forked Codex sessions from usage totals
- Keep separate Codex reasoning sections on separate lines

## [0.1.11]

- Highlight Markdown in the file editor, and toggle between source and a rendered preview
- Add UI and code font size settings
- macOS: Add "Open in.." button to open project folder in selected application

## [0.1.10]

- Add Kimi Code support
- Add Oh My Pi support
- Fix markdown table rendering

## [0.1.8]

- Fix `PATH` resolution on Windows

## [0.1.4]

- Fix text selection in diff view

## [0.1.3]

- Pin Codex and Claude commit message generation to cheap models: gpt-5.6-luna and claude-4.5-haiku
- Animate sidebars
- Render provider file edits as inline diffs in the transcript
- Fix claude task title generation

## [0.1.2]

- Fix regression: user bubble should fit its content width

## [0.1.1]

- Give nested Markdown the full message width
- Cap composer height and scroll overflow with an overlay scrollbar
- Keep drag-selecting text past the input bounds
- Fix char boundary panic when sliding the live reasoning window

## [0.1.0]

- Add standalone Goddard daemon and browser client
- Add Linux support (X11 and Wayland, you need to build from source for now)
- Answer agent questions directly in the composer
- Redesign queued follow-ups as composer cards with per-message steering
- Add DeepSeek agent preset selection (Standard, Code, Minimal, and Creator)
- Add Claude context window and ultracode effort options
- Add /fast command to toggle fast mode for Codex
- Show the latest activity in live transcript headers
- Add soft wrapping and keyboard copy feedback
- Add terminal overlay scrollbar and measure cell width from the font
- Restore window position, size, and display across launches
- Contain wheel scrolling in activity and command output viewports
- Smooth streaming markdown and reduce CPU usage while streaming

## [0.0.13]

- Add DeepSeek Harness provider
- Render user message as Markdown and linkify bare URLs
- Share one resident OpenCode serve per workspace across sessions

## [0.0.12]

- Inherit the login-shell environment for provider commands
- Fix model traits across provider switches
- Keep branch change counts current and include untracked files
- Normalize SIGCHLD for provider children
- Fix Grok model discovery

## [0.0.11]

- Fix provider detection for CLIs installed through shell PATH managers such as
  nvm and fnm
- Show models registered by Pi extensions
- Fix the model picker closing when entering a space in search
- Fix duplicate transcript history when resuming ACP sessions

## [0.0.10]

- Fix crash in due to IME composition
- Fix typo

## [0.0.9]

- Add OpenCode Go support in usage popover
- Fix app icon
- Fix Cursor model detection

## [0.0.8]

- Initial release
