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

## [0.3.0]

### Features

- New "After archiving a task" setting (Settings → General) picks where selection lands: the next unread completion (default), the next non-busy session in sidebar order, or a fresh task composer
- Add Antigravity as a provider: its sessions run the `agy` CLI's own TUI in a task-scoped terminal — the first prompt launches `agy -i`, reselecting a started session resumes with `agy --conversation`, and status and title come from agy's own store. The composer steps aside once the session starts so keystrokes belong to the TUI
- Archived chats now show a green checkmark and "Landed" label when their work was landed onto the base branch with `/land`
- New "Border intensity" slider (Settings → Appearance) fades borders and separators from invisible up to their original contrast — the default lands fainter than before
- Collapsed sidebar groups now show the same unread dot sessions and terminals use when any hidden row has an unseen completion
- Copying a selection from an agent message now puts markdown on the clipboard — `**bold**`, `` `code` ``, links, headings, list markers, quotes, and fenced code blocks — instead of flattened rendered text
- ⌘⇧. flips a draft's environment between this Mac and the sandbox VM — the Environment section of the ⌘. mode menu, without opening it
- The file viewer's image preview now pans on scroll and zooms around the cursor with ⌘-scroll
- The Keybinding Manager's table now edits bindings in a capture modal with one slot per command, ordered by surface: the on-screen keyboard wears platform glyphs, a tooltipped conflict count cycles through the offending commands, and only true conflicts are marked
- Landing a task now records a "Landed on `<base>`" card in its transcript listing the commits that landed — each SHA opens the commit diff — so where the work went stays visible in the session's record.
- On macOS 26, transparent chrome renders as real Liquid Glass (`NSGlassEffectView`): the sidebar and the chat composer card float on glass that lenses the desktop, tinted by the active theme. Older macOS keeps the existing vibrancy path unchanged.
- Add a "Mark all tasks as read" command-palette action that clears every unseen-completion dot at once
- Menus support the native press-drag-release gesture: hold a menu trigger (or right-click for a context menu), drag onto an item, and release to pick it — releasing over nothing dismisses
- Model picker is now one virtualized, filterable list with a jump rail instead of provider tabs: starred favorites lead (drag to reorder, ⌘⌥1–⌘⌥9 to apply), then recently used combos, then every provider's models — each row enumerates a model/effort/tier selection, with the provider mark on a second line and effort and fast mode drawn dimmer beside the name. Rail buttons clear the filter and scroll to that section, ⌘E / ⌘⇧E cycle reasoning effort for the current model in either direction, and packed CLI aliases (e.g. Cursor's `-high-fast` slugs) fold into their base model's effort and tier options
- "New task in…" (⌘⇧N, also in the File menu and command palette) fuzzy-searches directories on disk and starts a task in the pick — the directory becomes a temporary project that appears in the sidebar with a clock-folder icon and leaves the project list once its last task is removed; workspace toggle moves to ⌘⌥N and the branch picker to ⌘⇧⌥N
- Received-file sessions now default to the Sandbox VM environment, so once a transfer is trusted the agent still runs isolated from this Mac
- Rename sidebar terminals inline — double-click the title or pick Rename from the row's menu, just like sessions
- Tables can be resized by dragging a column boundary (or focusing it and using the arrow keys): markdown tables in the transcript, the Keybinding Manager, the Projects page's Worktrees and Branches lists, and the Usage breakdown tables
- “Create draft” parks the composer’s contents — text, attachments, and annotations — as a saved draft tied to its chat or project; a count button beside the composer’s access control opens the new Drafts page, where drafts can be searched, edited inline, hidden, deleted, or dropped back into their composer (⌘Z restores a used draft)
- Settings search now matches setting titles and descriptions across all pages: every section with a match renders in one scrollable list with the matching text highlighted, and the sidebar filters to those sections (clicking one scrolls to it)
- Sidebar project groups now cap at seven chats; the "Show more" row reveals the rest in batches of thirty, so an active project can't grow its section without bound
- Select multiple sidebar tasks with ⌘-click (⌘⇧-click extends the range) and act on them together — row menus and task shortcuts like pin, mark unread, archive, and copy working directory apply to the whole selection
- Sidebar transparency gains an Amount slider (Settings → Appearance): dial how much of the desktop shows through the sidebar, 0–60%. The default moves from a barely-visible tint to 25%.
- ⌘S now opens a "Sync branch…" picker outside the file editor (where ⌘S still saves). It lists the repository's checkouts that track an upstream — defaulting to the current branch in a session — and pulls with `git pull --rebase` (or merge, per the sync setting). Conflicts reuse the sync-conflict dialog, whose "Resolve in chat" now starts a new chat on the folder being synced. A new "Resolve sync conflicts in a chat" setting skips that dialog entirely: a pull or land that stops on conflicts starts a fresh chat on the checkout with the resolution steps already sent
- The Appearance settings gain a collapsible Preview row showing a miniature transcript — user bubble, assistant reply, and syntax-highlighted code — that opens automatically while a theme selector is open, so palette options can be compared on real content
- Closing the terminal you're viewing now selects a neighbor terminal instead of leaving a dead pane
- The "Task unarchived" toast now advertises ⌘⌥O — pressing it jumps straight to the restored task

### Experiments

- **[Experimental]** Move Computer Use behind an Experiments opt-in: turning the experiment on reveals the Computer Use settings page and lets sessions drive the bundled Cua Driver helper, including in release builds
- **[Experimental]** Move Friends — the friend-to-friend file transfer page — behind a Settings → Experiments toggle, off by default in release builds
- Harden friend transfers against lost control messages: a missed done receipt no longer fails a completed download or hides the received file, sends dial with timeouts and stall to Failed instead of sitting at 0 B, and failures log their cause
- Apply friend-request responses in one click — accepting mirrors the friend into the roster immediately instead of waiting for the protocol task, and repeat responds resolve as no-ops
- Add a customizable display name (what friends see) and per-friend local nicknames; completed transfers now also get a readable `Name-file` symlink under `~/Documents/Goddard/From Friends/`
- **[Experimental]** The GitHub integration moves behind a Settings → Experiments toggle — the inbox sidebar row, ⌘⇧I page, and notification polling all switch off until it's enabled — and gains two new surfaces: a "New GitHub Issue" command-palette action that scans `.github/ISSUE_TEMPLATE` and opens a native form for Markdown templates and blank issues (YAML forms hand off to the web), and `#`-completion in the composer that resolves issues and pull requests on the workspace's GitHub remote, drawing them as chips in the transcript
- **[Experimental]** The Git panel's top region — the commit box, or an open commit's file tree — is now split from the commit log by a draggable divider whose position persists, so opening a commit no longer shifts the layout
- **[Experimental]** Auto model routing: opt in under Settings → Experiments to add an Auto entry to the model picker. A task's first prompt is classified by the evaluation model and routed to a provider and model from your routing policy (~/.goddard/route-policy.json), with BYOK evaluation backends for TypeSafe, Vercel AI Gateway, and Cloudflare Workers AI and routine/general/demanding class-level target pickers. Class targets default to session-scoped tiers (`session:tier:*`), which upgrade or downgrade within the provider you already picked; global `tier:*` targets resolve through the policy's preferred-provider order instead.
- **[Experimental]** New Projects page (⌘⇧P, or the sidebar row beneath Search): each project gets Worktrees, Branches, Issues, and Pull Requests tabs (⌘⌥1–4) backed by the local repo and `gh`. Remote-tracking branches group under collapsible remotes that fetch on expand, rows follow macOS multi-selection with right-click menus and a bulk action bar, and a docked composer chips the current project, tab, filter, and selection

### Fixed

- On Linux, provider sessions no longer die at launch: the daemon's process guardian handed spawned CLIs `/dev/null` for stdin because dash assigns it before applying `<&0`, so stdio-based providers exited immediately — the child now inherits stdin through a saved descriptor
- `/land` no longer dead-ends on a plain local checkout — the command palette's "Land changes" only appears for worktree sessions, and running `/land` where it can't apply explains why instead of erroring
- Large pastes collapse less eagerly (20 lines or 4 KB, up from 4 lines or 500 bytes), and collapsed pastes now show as a compact "Pasted text" chip that previews the leading characters on hover.
- ⌘⇧N now reliably cycles the ⌘N project switcher backward — a fast press can no longer slip through to the draft's workspace toggle, and the chord opens the switcher in reverse on a draft too
- The sidebar peek no longer dismisses when archiving, pinning, or renaming a session from the hover-revealed sidebar
- Stopping a turn now stops its live background work at the provider too — a retained runtime no longer keeps running while the transcript claims it halted — and a steer sent just before Stop can no longer acknowledge into the settled session afterward
- ⌘Enter now steers a composer draft that only contains comment annotations, instead of doing nothing
- A global terminal now remembers its own right panel — switching away from a task no longer leaves the task's panel open over the terminal, and the panel's tabs and visibility return as they were left
- A sidebar terminal no longer resets into the selected task's worktree after a `cd` — session terminals only respawn when the workspace itself moves, and terminals opened at a chosen directory keep it

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
