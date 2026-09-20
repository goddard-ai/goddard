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

## [0.4.0]

### Features

- **Sessions**
  - Archiving a task now shows a "Task archived" toast with an Undo button that restores it — a multi-selection archives as one undoable group
  - Sessions can be swept into dormancy: hold Option while hovering a session and its pin button becomes a broom, or use "Move to Dormant" in the row's context menu (works on multi-selections too). Tasks with no activity for 7 days go dormant automatically — pinned and in-progress tasks are exempt, a task with uncommitted work keeps its worktree on disk, and the threshold is adjustable under Settings → General ("Dormant after", including a "Never" option). Under Date grouping dormant tasks collect in a collapsed "Dormant" group at the end of the sidebar; under Project grouping they stay inside their project, always below the fold — once every live session is revealed, "Show more" becomes "Show dormant". Dormant tasks stay until you archive them: any new activity, or "Restore" from the context menu, wakes them back into the list
  - Opening a task with unseen completions now shows an accent dot beside the first line of the agent's latest reply, fading out on the first scroll
  - `/side [prompt]` opens a side chat in the task's right panel — a fresh session linked to the task that can read its transcript with `goddard-agent read` and message it with `goddard-agent prompt`. Side chats live as tabs in the panel, out of the sidebar and switcher, resume across restarts, and are deleted with their parent task.
  - Selecting a task now reopens its transcript at the scroll position you left it at instead of jumping to the last prompt; tasks with unread completions or a pending question still open on their last turn.
- **Composer**
  - Typing a quote, backtick, or bracket over a text selection in the composer now wraps the selected text in the matching pair instead of replacing it
  - Collapsed text pastes now fold into the composer input as "Pasted text" chips at the paste position instead of collecting above it; pastes over 64 KB still become `paste.txt` file attachments
- **Providers**
  - ⌥Tab cycles the composer session through its favorited model+effort combos plus the most recently used selection
  - Goose is now a supported provider, driven over the Agent Client Protocol (`goose acp`). Goose sessions stream, prompt for permissions, and can be imported into Goddard like other ACP providers; the model comes from your `goose configure` setup.
  - The model picker's search field understands structured tokens — `provider:<id>` (e.g. `provider:pi`) and `effort:<id>` — which filter on the row's fields and wash the recognized value so a working token reads differently from a mistyped one; clicking a provider in the rail now toggles its `provider:` token instead of scrolling, while the favorites and recents buttons still jump to their sections
  - Every provider's model catalog now folds exploded aliases — `swe-2-high` and `swe-2-high-fast` listed next to (or instead of) `swe-2` — into one base row with a strength-ordered effort ladder and fast tier, the same fold the model picker already applied to Cursor's CLI slugs
- **Git**
  - New "Resolve land conflicts in chat" setting (Settings → General) sends the conflict-resolution prompt to the owning task's chat automatically when a land stops on rebase or merge conflicts, instead of showing the conflict dialog
  - Rank branch picker results by a blend of name fit and last-committed recency; with no search text the list now orders by most recently committed
  - Link commit hashes in agent messages: hovering shows the commit card, and clicking opens the commit's diff in the Git panel (requires the Git panel experiment)
  - Drag the divider between an open commit's diff and its file tree in the Git panel to resize the split; it adjusts the same panel width as the panel's own edge
  - The new-task sync strip now reads Pull changes when the checkout trails upstream and offers Push changes when it only leads, instead of always saying Sync changes
- **Navigation**
  - The Archived page's search now matches message text inside archived chats, not just titles and project names — rows found on content show a snippet of the matched line
  - The command palette gains a Prompts section listing every discovered prompt template: picking one with an argument hint inserts `/name ` for the usual submit flow, and any other inserts the expanded body so the prompt is visible and editable before it sends. "Save as prompt template" parks the composer draft and writes it to `~/.config/goddard/commands/<slug>.md`, and "Open prompts folder" keeps management in the markdown files that are already the source of truth
- **Permissions**
  - New tasks now default to Auto-accept edits instead of Full access — file edits apply without prompts while commands and other actions still ask first
  - Switching to Full access for the first time now shows a one-time confirmation explaining that the agent can run commands and edit files without approval prompts
- **Settings**
  - Settings → Daemon now shows the commit the app and the connected daemon were each built from, so a stale app/daemon pairing is visible at a glance
  - The settings sidebar now behaves like the chat sidebar: it shares the same width, drags wider or narrower from its right edge, and picks up the macOS sidebar transparency (vibrancy or Liquid Glass) when that setting is on.
- **Friends**
  - Friends and file transfers now find direct LAN paths through mDNS endpoint discovery instead of routing through relays, working even with the internet down
  - Manage pairings under Settings → Friends: pending requests show the requesting device with approve and decline controls, and the paired-devices list can revoke any device's token independently
- ⌘W on a terminal filling the main area kills it — with a confirmation first while a command is still running inside
- Keybindings search now filters by the keys themselves: typing a modifier in any spelling — "cmd", "command", "opt", "option", "ctrl", "control", "shift", "shft", "fn", or the ⌘⌥⌃⇧ glyphs — lists every binding that uses it, key names like "tab" or "f5" match too, and combinations such as "command p" narrow to the chords using them all.
- Discover and pair with daemons on the local network: an exposed daemon now announces itself over Bonjour and the share endpoint's encrypted link, Settings → Daemon lists nearby daemons with a Pair button, and the mobile app's daemon list shows an "On your network" section — approving a request on the host issues a revocable per-device token, so adding a machine no longer means copying an address and token by hand
- On macOS 26, the transparent sidebar renders as real Liquid Glass (`NSGlassEffectView`) lensing the desktop; older macOS keeps the existing vibrancy path unchanged. The transparency slider now drives the sidebar's translucent fill over the material — solid through frosted glass to a bare lens at 100%. Context menus and dropdowns get a Liquid Glass treatment too — a translucent card with a specular top sheen over whatever lies beneath
- Opening the right panel (⌘⌥B or the panel toggle) moves keyboard focus into the active surface — the terminal, browser, file editor, or diff file tree — so its keybindings (⌘R reload, ⌘± code zoom, find toggles) work from the first keystroke; surfaces with no focusable body focus the panel itself
- New "Sidebar draft previews" setting (General) shows a task's unsent composer draft on its own line under the sidebar row's title, drawn in the theme's alert color so an abandoned draft stands out

### Experiments

- **Friends**
  - **[Experimental]** Add project sharing between friends: pick which projects a friend can see, and when their share matches one of your repos by `origin` remote you can opt in to automatic sync — the default branch syncs out of the box, other branches are per-branch toggles, and commits you land auto-push (toggleable). A friend's push syncs immediately; a rebase conflict or dirty checkout raises an alert offering resolve-in-chat, merge instead, or abort — aborting pauses that branch until you sync it manually.
  - Attach an optional message when sending a file or folder to a friend — the receiver's chat shows the note above the delivered file as an agent-style message, inherits their last-used provider/model, and stays provider-selectable until they prompt it; the quarantined session's composer stays live: Enter on the empty composer hands the files and note to the agent while the transfer stays quarantined until trusted
  - Opt in to session sharing per shared project: a "Share sessions" toggle under each outgoing share lets a friend list that project's sessions and peek into any of them — live and read-only. Watched sessions show a banner naming the friend, never touch the friend's runtime, and disappear from view the moment sharing is revoked.
  - Move Friends — the friend-to-friend file transfer page — behind a Settings → Experiments toggle, off by default in release builds
  - Harden friend transfers against lost control messages: a missed done receipt no longer fails a completed download or hides the received file, sends dial with timeouts and stall to Failed instead of sitting at 0 B, and failures log their cause
  - Apply friend-request responses in one click — accepting mirrors the friend into the roster immediately instead of waiting for the protocol task, and repeat responds resolve as no-ops
  - Add a customizable display name (what friends see) and per-friend local nicknames; completed transfers now also get a readable `Name-file` symlink under `~/Documents/Goddard/From Friends/`
- **[Experimental]** Reviewed Auto access: the Auto access mode now sends provider permission requests through the configured evaluation backend (Settings → Evaluation) instead of blanket-approving them. A `clear` verdict allows the action once — never a durable grant — while caution, an unreachable backend, or an unreadable answer escalates to the normal permission prompt, so Auto without credentials degrades to ask-the-user. Providers with their own review (Codex, Claude, fx, Droid) keep their native escalations.
- **[Experimental]** Automations: opt in under Settings → Experiments for an Automations page (⌘⇧U) that runs prompts on a trigger — hourly, daily, weekdays, weekly, a custom cron expression with an optional IANA timezone, or a webhook URL that fires a run on each POST — running each into a fresh task, a new worktree, or an existing one, with a run history that links every run back to its task.
- **[Experimental]** MCP integrations: a new Settings → Integrations page (Experiments opt-in) connects official remote MCP servers — Linear, GitHub, Notion, Stripe, Figma, Supabase, and more — to the agents you choose. Goddard owns sign-in (OAuth or API key, credentials in the keychain) and serves every agent through its local MCP proxy, so providers never see upstream credentials. Connect once, any provider.
- **[Experimental]** Auto model routing gains a dedicated Settings → Jev page for the backend credentials and routing targets — reachable from the Auto row's gear button, which shows a warning badge while the selected backend has no API key. Each task class (Routine/General/Demanding) maps to a user-picked provider, model, and reasoning effort, and Jev can suggest mappings from recent usage. The routed model locks in at session start while Jev reevaluates the reasoning effort on every turn, changing it only when confident
- **[Experimental]** Project map: opt in under Settings → Experiments to give every new session a compact structural map of its workspace — the top-level symbols of the most-referenced files, budgeted near 1,000 tokens and prepended to the first prompt — so the agent skips cold repo exploration. A composer chip tracks indexing, and the sent map lands as a collapsible transcript row.
- **[Experimental]** Project memory: each project keeps a `.goddard/memory/` store (a curated `MEMORY.md` plus an append-only `LOG.txt`) that the daemon maintains automatically — finished sessions are distilled into durable notes in the background, and the store is injected into every new session's first prompt so agents start with prior decisions, preferences, and failed approaches instead of zero context. The store is excluded from git via `.git/info/exclude`, never appears in the transcript, and can be inspected or edited directly.
- **[Experimental]** QA review: proposed work lands on `qa` unreviewed and promotes to the base branch through the longest approved prefix. The Projects page gains a Review tab listing the queue oldest-first — `Test-Plan:` trailers and sensitive paths mark commits that need a human while everything else auto-approves — with per-commit approve, reject-and-revert, and a promote control for the approved frontier. Decisions live in `refs/notes/qa` and sync to friends over the existing share path.
- **[Experimental]** Sidebar dock: opt in under Settings → Experiments, then hover anywhere along the sidebar's bottom strip to raise a quick-action dock — Inbox, Archived chats, Shortcuts, and Settings (plus Friends when its experiment is on) as round buttons that slide up from below the window's bottom edge to overlap the strip, magnify toward the pointer like the macOS Dock, slide back down when the pointer leaves, and stay raised while the pointer is on them even when a button opens Settings
- **[Experimental]** Turn status markers: opt in under Settings → Experiments to have the evaluation model score each finished assistant turn against a fixed marker set — complete, awaiting input, unverified, partial, blocked, failed, drifted — and show the markers that clear their threshold as chips with a confidence percentage under the response. Evaluations run only for the session on screen; turns that end off screen are scored when you open it. Requires a configured evaluation backend, shared with Auto model routing.

### Fixed

- **Sidebar**
  - Archiving a task mid-⌘⇧D sweep no longer lands selection back on a task the sweep just marked unread; the departure falls through to the next genuinely unread or idle session until another navigation ends the sweep
  - Holding ⌘ no longer shows the ⌘1–⌘9 row chips while a sidebar multi-selection exists, so the hints don't advertise a single-task jump over an active batch
  - Holding ⌘ for the ⌘1–9 sidebar chips no longer hides a session row's status icon, pin/archive controls, worktree and PR badges, or timestamp — the chip is now a pure overlay and the row underneath stays put
  - Folding the sidebar's Terminals group now returns to where you were — the fold skips every intermediate terminal in the back stack instead of landing on the previously viewed terminal
- **Providers**
  - Fix ACP tasks losing their conversation after Move to Worktree: a failed `session/load` — e.g. while the replaced runtime still held the provider's session lock — silently started a fresh provider session and overwrote the resume cursor. Failed resumes now retry transient errors briefly and surface the failure instead of forking onto an empty session
  - Antigravity's model list folds `base-effort` spellings like `gemini-3.1-pro-high` into one model row with an effort picker and drops the parenthesized effort from the label ("Gemini 3.1 Pro"), while traitless suffixes like `claude-opus-4-6-thinking` keep their full id instead of resolving to a base the CLI rejects
  - Selecting a Devin model no longer fails with "did not advertise model": the picker stores the folded base id with effort and fast as separate traits while Devin's ACP session advertises packed uids, and the stored traits are now repacked into candidate ids before matching
  - The model picker's rail no longer shows buttons for sections with no rows — other providers during a locked session, providers whose combos are all favorites or recents, and favorites/recents jumps whose stored entries no longer resolve to picker rows
  - OpenCode sessions no longer fail to start when the session server is slow to boot — its startup budget is now 30 seconds, up from 10
  - Detect provider CLIs installed after the daemon started and ones on PATH only via interactive shell config: provider refresh re-captures the login-shell environment, and a missed binary falls back to a single `command -v` resolution across all providers
  - Stop now interrupts an in-flight Computer Use `js` call on every provider: Goddard drops a `cancel-kernel` marker into the session's process directory that the QuickJS kernel polls, since its synchronous serve loop can never see an MCP `notifications/cancelled`. Stopping a Codex turn also keeps the app-server resident like every other provider instead of ending the process
  - Fixed Vercel AI Gateway evaluation calls failing with HTTP 400: `noul` questions are now translated to the spec's `boolean` type (and back on answers), the answering model id is read from gateway routing metadata, and the request no longer forces Zero Data Retention, which requires a Pro or Enterprise plan.
- **Git**
  - Re-picking a draft's base branch no longer runs `git switch` or disables branches owned by other worktrees — the draft's detached worktree is re-pointed in place, so every branch stays selectable
  - Archiving a chat no longer deletes its worktree while another unarchived chat shares it; the directory is removed once the last sibling chat is archived.
  - Syncing a checkout from the new task strip can no longer open a Git editor in the terminal tab — the pull now runs with `core.editor` and `sequence.editor` stubbed out
- **Panels**
  - Clicking a file row in a changed-files card now opens that file in the Review panel's turn diff instead of the file viewer, matching the Review button's destination
  - Keep the sidebar visible when the right panel is maximized — the layer now covers only the session column, and toggling the sidebar resizes the layer to match
- **Appearance**
  - In High contrast mode, the chevron carets on select-style controls and the disclosure chevrons on expandable rows (settings sections, transcript folds, menus) now render one text tier brighter so the affordance stays legible.
  - Browsing themes in the Appearance selectors no longer flashes back to the current theme a moment after each preview while "Match system appearance" is on
- The composer's access-mode rows now read but can't be picked while a turn is running — previously switching the mode mid-turn restarted the driver and silently cancelled the turn
- Fixed `~/Documents/Goddard/From Friends` symlinks pointing at the `transfers/<uuid>` folder instead of the received file itself.
- Shrunk the installer back to its old size — the Copilot SDK's default `bundled-cli` feature was embedding the entire ~118 MB Copilot CLI archive in the daemon even though sessions always launch the installed `copilot` binary, which had grown the download from ~35 MB to ~176 MB
- The Keybinding Manager no longer reports permanent unresolvable conflicts for declared fall-through pairs like ⌘N/⌘⇧N — bindings whose winner propagates the keystroke to the next command when its runtime gate fails
- Connecting to a remote host over SSH works again: the control socket option was passed in a form `ssh` rejected outright, the control master was started in confirmation mode so every later command and port forward was denied, and a host with no published daemon build failed right after the bundled daemon was uploaded.

## [0.3.0]

### Features

- **Sessions**
  - New "After archiving a task" setting (Settings → General) picks where selection lands: the next unread completion (default), the next non-busy session in sidebar order, or a fresh task composer
  - Archived chats now show a green checkmark and "Landed" label when their work was landed onto the base branch with `/land`
  - Add a "Mark all tasks as read" command-palette action that clears every unseen-completion dot at once
  - The "Task unarchived" toast now advertises ⌘⌥O — pressing it jumps straight to the restored task
- **Sidebar**
  - Collapsed sidebar groups now show the same unread dot sessions and terminals use when any hidden row has an unseen completion
  - Sidebar project groups now cap at seven chats; the "Show more" row reveals the rest in batches of thirty, so an active project can't grow its section without bound
  - Select multiple sidebar tasks with ⌘-click (⌘⇧-click extends the range) and act on them together — row menus and task shortcuts like pin, mark unread, archive, and copy working directory apply to the whole selection
- **Providers**
  - Add Antigravity as a provider: its sessions run the `agy` CLI's own TUI in a task-scoped terminal — the first prompt launches `agy -i`, reselecting a started session resumes with `agy --conversation`, and status and title come from agy's own store. The composer steps aside once the session starts so keystrokes belong to the TUI
  - Model picker is now one virtualized, filterable list with a jump rail instead of provider tabs: starred favorites lead (drag to reorder, ⌘⌥1–⌘⌥9 to apply), then recently used combos, then every provider's models — each row enumerates a model/effort/tier selection, with the provider mark on a second line and effort and fast mode drawn dimmer beside the name. Rail buttons clear the filter and scroll to that section, ⌘E / ⌘⇧E cycle reasoning effort for the current model in either direction, and packed CLI aliases (e.g. Cursor's `-high-fast` slugs) fold into their base model's effort and tier options
- **Git**
  - Landing a task now records a "Landed on `<base>`" card in its transcript listing the commits that landed — each SHA opens the commit diff — so where the work went stays visible in the session's record.
  - ⌘S now opens a "Sync branch…" picker outside the file editor (where ⌘S still saves). It lists the repository's checkouts that track an upstream — defaulting to the current branch in a session — and pulls with `git pull --rebase` (or merge, per the sync setting). Conflicts reuse the sync-conflict dialog, whose "Resolve in chat" now starts a new chat on the folder being synced. A new "Resolve sync conflicts in a chat" setting skips that dialog entirely: a pull or land that stops on conflicts starts a fresh chat on the checkout with the resolution steps already sent
- **Transcript**
  - Copying a selection from an agent message now puts markdown on the clipboard — `**bold**`, `` `code` ``, links, headings, list markers, quotes, and fenced code blocks — instead of flattened rendered text
  - Tables can be resized by dragging a column boundary (or focusing it and using the arrow keys): markdown tables in the transcript, the Keybinding Manager, the Projects page's Worktrees and Branches lists, and the Usage breakdown tables
- **Terminals**
  - Rename sidebar terminals inline — double-click the title or pick Rename from the row's menu, just like sessions
  - Closing the terminal you're viewing now selects a neighbor terminal instead of leaving a dead pane
- **Keyboard**
  - The Keybinding Manager's table now edits bindings in a capture modal with one slot per command, ordered by surface: the on-screen keyboard wears platform glyphs, a tooltipped conflict count cycles through the offending commands, and only true conflicts are marked
  - Menus support the native press-drag-release gesture: hold a menu trigger (or right-click for a context menu), drag onto an item, and release to pick it — releasing over nothing dismisses
- **Appearance**
  - New "Border intensity" slider (Settings → Appearance) fades borders and separators from invisible up to their original contrast — the default lands fainter than before
  - On macOS 26, transparent chrome renders as real Liquid Glass (`NSGlassEffectView`): the sidebar and the chat composer card float on glass that lenses the desktop, tinted by the active theme. Older macOS keeps the existing vibrancy path unchanged.
  - Sidebar transparency gains an Amount slider (Settings → Appearance): dial how much of the desktop shows through the sidebar, 0–60%. The default moves from a barely-visible tint to 25%.
  - The Appearance settings gain a collapsible Preview row showing a miniature transcript — user bubble, assistant reply, and syntax-highlighted code — that opens automatically while a theme selector is open, so palette options can be compared on real content
- ⌘⇧. flips a draft's environment between this Mac and the sandbox VM — the Environment section of the ⌘. mode menu, without opening it
- The file viewer's image preview now pans on scroll and zooms around the cursor with ⌘-scroll
- "New task in…" (⌘⇧N, also in the File menu and command palette) fuzzy-searches directories on disk and starts a task in the pick — the directory becomes a temporary project that appears in the sidebar with a clock-folder icon and leaves the project list once its last task is removed; workspace toggle moves to ⌘⌥N and the branch picker to ⌘⇧⌥N
- Received-file sessions now default to the Sandbox VM environment, so once a transfer is trusted the agent still runs isolated from this Mac
- “Create draft” parks the composer’s contents — text, attachments, and annotations — as a saved draft tied to its chat or project; a count button beside the composer’s access control opens the new Drafts page, where drafts can be searched, edited inline, hidden, deleted, or dropped back into their composer (⌘Z restores a used draft)
- Settings search now matches setting titles and descriptions across all pages: every section with a match renders in one scrollable list with the matching text highlighted, and the sidebar filters to those sections (clicking one scrolls to it)

### Experiments

- **Git**
  - **[Experimental]** The GitHub integration moves behind a Settings → Experiments toggle — the inbox sidebar row, ⌘⇧I page, and notification polling all switch off until it's enabled — and gains two new surfaces: a "New GitHub Issue" command-palette action that scans `.github/ISSUE_TEMPLATE` and opens a native form for Markdown templates and blank issues (YAML forms hand off to the web), and `#`-completion in the composer that resolves issues and pull requests on the workspace's GitHub remote, drawing them as chips in the transcript
  - **[Experimental]** The Git panel's top region — the commit box, or an open commit's file tree — is now split from the commit log by a draggable divider whose position persists, so opening a commit no longer shifts the layout
  - **[Experimental]** New Projects page (⌘⇧P, or the sidebar row beneath Search): each project gets Worktrees, Branches, Issues, and Pull Requests tabs (⌘⌥1–4) backed by the local repo and `gh`. Remote-tracking branches group under collapsible remotes that fetch on expand, rows follow macOS multi-selection with right-click menus and a bulk action bar, and a docked composer chips the current project, tab, filter, and selection
- **Friends**
  - **[Experimental]** Move Friends — the friend-to-friend file transfer page — behind a Settings → Experiments toggle, off by default in release builds
  - Harden friend transfers against lost control messages: a missed done receipt no longer fails a completed download or hides the received file, sends dial with timeouts and stall to Failed instead of sitting at 0 B, and failures log their cause
  - Apply friend-request responses in one click — accepting mirrors the friend into the roster immediately instead of waiting for the protocol task, and repeat responds resolve as no-ops
  - Add a customizable display name (what friends see) and per-friend local nicknames; completed transfers now also get a readable `Name-file` symlink under `~/Documents/Goddard/From Friends/`
- **[Experimental]** Move Computer Use behind an Experiments opt-in: turning the experiment on reveals the Computer Use settings page and lets sessions drive the bundled Cua Driver helper, including in release builds
- **[Experimental]** Auto model routing: opt in under Settings → Experiments to add an Auto entry to the model picker. A task's first prompt is classified by the evaluation model and routed to a provider and model from your routing policy (~/.goddard/route-policy.json), with BYOK evaluation backends for TypeSafe, Vercel AI Gateway, and Cloudflare Workers AI and routine/general/demanding class-level target pickers. Class targets default to session-scoped tiers (`session:tier:*`), which upgrade or downgrade within the provider you already picked; global `tier:*` targets resolve through the policy's preferred-provider order instead.

### Fixed

- **Composer**
  - Large pastes collapse less eagerly (20 lines or 4 KB, up from 4 lines or 500 bytes), and collapsed pastes now show as a compact "Pasted text" chip that previews the leading characters on hover.
  - ⌘Enter now steers a composer draft that only contains comment annotations, instead of doing nothing
- **Providers**
  - On Linux, provider sessions no longer die at launch: the daemon's process guardian handed spawned CLIs `/dev/null` for stdin because dash assigns it before applying `<&0`, so stdio-based providers exited immediately — the child now inherits stdin through a saved descriptor
  - Stopping a turn now stops its live background work at the provider too — a retained runtime no longer keeps running while the transcript claims it halted — and a steer sent just before Stop can no longer acknowledge into the settled session afterward
- **Terminals**
  - A global terminal now remembers its own right panel — switching away from a task no longer leaves the task's panel open over the terminal, and the panel's tabs and visibility return as they were left
  - A sidebar terminal no longer resets into the selected task's worktree after a `cd` — session terminals only respawn when the workspace itself moves, and terminals opened at a chosen directory keep it
- `/land` no longer dead-ends on a plain local checkout — the command palette's "Land changes" only appears for worktree sessions, and running `/land` where it can't apply explains why instead of erroring
- ⌘⇧N now reliably cycles the ⌘N project switcher backward — a fast press can no longer slip through to the draft's workspace toggle, and the chord opens the switcher in reverse on a draft too
- The sidebar peek no longer dismisses when archiving, pinning, or renaming a session from the hover-revealed sidebar

## [0.2.1]

### Features

- **Providers**
  - Add a real compact command: the daemon routes compaction through the provider instead of faking it with a prompt, and Codex and OpenCode 2 show context compaction as its own activity card
  - Add GitHub Copilot as a provider via the official Rust SDK, with rewind and branch through `sessions.fork` and composer attachments sent as structured attachments
- **Git**
  - The Projects page's docked composer gains the chat footer's project, worktree, and branch pickers and a working model selector, so a task is fully configured before it is created
  - New opt-in setting fast-forwards the local default branch — plus any extra branches you list — to its tracking branch before a new worktree is based on it
- **Panels**
  - Preview images and SVGs in the file viewer — refreshed from just-saved bytes — and jump to a line with ctrl-g
  - Open @-mention file references from user prompts in the right panel
- **Terminals**
  - Rebind ⌘T to always open a new terminal rooted in the current context — the on-screen terminal's directory, the selected session's workspace, or ~ — moving the Terminals chord to ⌘⇧T and leaving Toggle Workspace reachable from the menu, command palette, and Work-in chip; ⌘⇧N now toggles the workspace on the new task page
  - Sidebar terminal rows no longer show a checkmark when a command exits cleanly; a terminal that finishes while off-screen now gets an unread dot that clears when the terminal is next focused
- **Navigation**
  - Jump to file:line[:column] straight from the ⌘P finder
  - Retarget ⌘D as "Go to next unread completion": it always picks the topmost unread row, cycles idle sessions once the unread queue drains, skips sessions with queued prompts, and ⌘⇧D is now a positional sweep instead of a next-unread jump
- **Appearance**
  - New "High contrast borders" setting (Appearance) widens border-tier contrast so control outlines meet WCAG's 3:1 non-text floor; it turns on automatically when the OS Increase Contrast accessibility setting is enabled
  - Borders, separators, and outlines are softer by default — hairline rules sit at 1.4:1 and filled surfaces like the composer, cards, menus, and tooltips use a new subtler outline tier
- Archiving a task whose turn is still running now asks for confirmation first instead of silently stopping the turn
- Add a Settings → Friends page for friend-to-friend file transfers: probe-based presence, incoming requests you accept or deny, live byte progress with a sidebar footer ring, and received sessions materialized under a synthetic Friends project behind an explicit trust quarantine
- Rename the product to Goddard and migrate legacy Waku state
- Add a keybinding manager page — a searchable command catalog over a keyboard stage — with a capture editor for live rebinding, conflict surfacing gated behind an explicit confirm, a manual keyboard-layout picker, and overrides persisted to keybindings.json and applied at startup
- Connect to remote hosts over SSH — managed from daemon settings, badged in the sidebar, with masked-password askpass prompts and port forwarding — so their sessions, skills, and usage appear alongside local tasks, and the daemon installs and upgrades itself on the host
- Add a sandbox environment choice to the composer's access menu
- Move worktree and branch settings into a dedicated Settings → Git page
- Cap each sidebar project group at sixteen chats; a "Show more" row reveals the rest in batches instead of folding only chats older than three days

### Experiments

- **[Experimental]** The Projects page (⌘⇧P) — a project's worktrees, branches, issues, and pull requests in one place — moves behind an Experiments opt-in and is now off by default

### Fixed

- **Sidebar**
  - Use the session's display title when dragging it in the sidebar
  - Keep worktree badges, branch labels, and checkout status on sidebar tasks across restarts instead of waiting for each task to be resumed
- **Git**
  - Detect rebase state directories in the git panel instead of relying on a stale REBASE_HEAD
  - `/land` shows a spinner toast while it runs and resolves it to the outcome, instead of giving no feedback until the land finished
  - "Resolve in chat" in the git panel conflict modal now pastes the resolution prompt into the composer instead of sending it, so you can review or edit it before sending
- **Transcript**
  - Round markdown table header corners to match the frame
  - Render backticks glued to shortcut keycaps as literal keycaps instead of breaking inline code
- **Keyboard**
  - Git panel confirmation modals, agent permission prompts, and computer-use approvals are now keyboard-operable — Tab moves between options, Enter or Space activates the focused one, Enter on a modal card runs its primary action, and Escape cancels or denies
  - Restore the ⌘N hint on New Task surfaces
  - Drop the unbound Toggle Workspace row from the shortcuts dialog
- **Appearance**
  - Diff highlights and the scrollbar in the file-diff hover cards no longer paint past the card's rounded bottom corners
  - Paint the sidebar solid when macOS Reduce Transparency is on, and default sidebar transparency off on setups where no backdrop blur exists
- Edit the annotation under an ⌥-click instead of stacking a new one on top
- Computer Use no longer fails with "socket path is too long": its bridge socket now lives in a private directory under `/tmp`, short enough for macOS's 104-byte `sun_path` limit regardless of `TMPDIR`

## [0.2.0]

### Features

- **Sessions**
  - Continue an interrupted session by typing in its empty composer
  - Play a sound when a background task finishes its turn, with a volume slider and in-selector previews, and show an unseen-completion bell in the top bar
  - Recover renamed or moved project folders instead of losing them, and archive projectless workspaces automatically
- **Sidebar**
  - Peek at the closed sidebar by hovering the left window edge
  - Pin, archive, and mark sessions unread from the sidebar, drag sessions into the composer as reference chips, and jump between tasks with ⌘1–9, ⌘D, and ⌘⇧D
- **Composer**
  - Annotate transcript lines and file-editor selections with comments that fold into the next prompt (⌥-click a line, ⌘L on a selection); "Annotation N" references resolve to hover citations
  - Highlight and auto-continue Markdown lists in the composer, collapse large pastes into expandable cards, and accept file drops anywhere in the session
- **Git**
  - Add a Projects page listing worktrees, branches, issues, and pull requests
  - Create named adjective-noun worktrees beside the repository, move a session into a new worktree, and remove an archived task's worktree behind a snapshot ref
- **Terminals**
  - Run project scripts from a ⌘R picker and generate terminal commands with the session's agent
  - Add a Terminals group to the sidebar showing live command status, working directory, and last-activity time; ⌘J focuses the session terminal
- **Navigation**
  - Add user-defined custom commands to the command palette — with custom icons and toast notifications — and let agents manage them through daemon settings
  - Open workspace files in the right panel with a ⌘P file finder and preview Markdown files fullscreen
  - Reshape the ⌃⇥ task switcher into a compact recently-viewed list with session status glyphs
- **Appearance**
  - Add UI and code font family pickers and a separate terminal font size to Appearance settings; right-panel cards adapt cleanly when the UI font size is increased
  - New settings: thick borders, sidebar transparency, a Markdown preview toggle, and an opt-in response token speed readout
  - Add 15 new themes (Dracula, Rosé Pine, Kansō, Gruvbox, GitHub, and more), split the theme preference into separate light and dark slots with a system-following toggle, and preview themes while browsing the selectors
- Add an Experiments settings page where unfinished features — Big Picture, the Git panel, GitHub integration, and subagents — can be turned on individually; all default off
- Hide the app with Cmd+H on macOS
- Add Devin and Droid (Factory) as agent providers
- Surface keyboard shortcuts in menus, tooltips, the command palette, a hold-⌘ overlay, and a cheatsheet next to the sidebar settings icon
- Syntax-highlight Lua, PHP, Zig, Dart, Elixir, and Astro code blocks

### Experiments

- **Git**
  - **[Experimental]** Add a Git panel (⌘⌥G) with a commit graph, expandable diffs, upstream tracking, and a /land command to land a worktree on its base
  - **[Experimental]** Browse a project's GitHub issues and pull requests, start tasks from them, and track check and review status on sidebar rows and in the right panel
- **[Experimental]** Add Big Picture mode on ⌘0 — a full-window grid of session cards with live transcripts, per-card composers, and ⌘1–9 arming
- **[Experimental]** Delegate runs to named subagents with per-provider tiers and cost-labeled routing, shown legibly in the transcript

### Fixed

- **Providers**
  - Keep Amp threads resumable after restarting the app
  - Fix Cursor model discovery and model options
  - Apply OpenCode model and effort changes to the live session instead of the next one
- **Git**
  - Prefer exact matches in the branch selector's filter
  - Stop warning about unpushed commits that are already merged into a branch
- **Transcript**
  - Fix escaped backticks rendering literally inside inline code
  - Stop rendering blank reasoning-only lines in the transcript
  - Fix remote images failing to load in transcripts
  - Fix the transcript segment left behind when steering an in-flight reply
- **Platform**
  - Fix Waku→Goddard migration staging a full copy of `~/.waku` on every launch: workspaces, worktrees, and archives are now linked item-by-item instead of copied, the state database is cloned only when its schema is known, and abandoned `.migrating-*` staging directories are swept on launch
  - Fix startup on Macs without Xcode installed
- Fix the IME candidate popup appearing in the wrong position
- Stop a quick ⌃⇥ chord from flashing the task switcher
- Fix interrupted session saves erasing stored workspace details

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
