# Goddard Wiki

Everything a user might want to know about Goddard: what it is, what it does,
how it works, and whether it fits the way you want to work.

## What is Goddard?

Goddard is a fast, native desktop app for working with local coding agents.
Instead of living inside one agent's terminal UI, you run **Claude Code, Codex,
Cursor, OpenCode, and nine other agent CLIs** side by side from one interface —
each task gets a real transcript, a diff view, a terminal, a file browser, and
Git tooling, while the agent CLI does the actual work underneath.

(If you knew it as **Waku** — same app, renamed. That's why download URLs say
`goddardai.org` and internal pieces are named `waku-*`.)

It is written in Rust on top of [GPUI](https://github.com/zed-industries/zed)
(the framework that powers the Zed editor), which is why it stays smooth on
long transcripts and high-refresh displays where Electron and web clients tend
to stutter.

**Goddard is not an agent.** It does not have its own model or subscription.
It drives the agent CLIs you already have installed and authenticated, over
each provider's native session protocol — so your existing Claude, ChatGPT,
Cursor, or other agent subscription is what you pay with.

### Quick facts

| Question | Answer |
| --- | --- |
| Price | Free, open source (GPL v3) |
| Account required | No Goddard account. You sign in through each agent's own CLI |
| Platforms | macOS (signed `.dmg`), Linux (install script, Wayland + X11), Windows (per-user installer or portable zip) |
| Download size | ~80 MB |
| Where is my data | Locally on your machine — no Goddard cloud service |
| Languages | English, Japanese, Simplified Chinese |
| Updates | Automatic, cryptographically signed, with rollback on Linux |
| Community | GitHub issues and Discord (both linked from the site menu) |

## Is Goddard right for you?

**Goddard is a good fit if you:**

- Run coding agents daily and juggle more than one task or project at a time.
- Use more than one agent CLI and want a single consistent interface — one
  model picker, one permission system, one transcript — instead of learning
  each CLI's TUI.
- Want to queue follow-up messages or steer a running agent instead of waiting
  for it to finish.
- Want conversation-aware rewind and branching that also rolls back the Git
  working tree.
- Care about keyboard-driven workflows, native performance, and a UI that
  doesn't drop frames on a 120 Hz monitor.
- Want your code, sessions, and transcripts to stay on your machine.

**It may not fit if you:**

- Don't already use a coding-agent CLI — Goddard needs at least one installed.
- Need a hosted/cloud agent that runs while your laptop is closed (Goddard
  runs agents on your machine or a daemon host you control).
- Rely on a specific agent's IDE extension features (inline completions in an
  editor, etc.) — Goddard is a task-and-transcript app, not a code editor.

## Recently shipped

The app is under heavy development — the last four days alone landed ~220
commits. Highlights, grouped by area:

- **GitHub:** an in-app pull-request and issue browser per project, PR state
  with check/review status on sidebar rows, and starting a task straight from
  a PR or issue (including "fix failing checks").
- **Git and worktrees:** a Git panel (stage, commit, push, sync with
  rebase-or-merge), moving a local task into a named worktree, worktree state
  preserved across archiving, and a sync notice when a new task's checkout
  trails upstream.
- **Terminals:** a sidebar Terminals group with repo-named rows, live cwd and
  command status from shell integration, ⌘J focus, ⌘⇧K scrollback clear, a
  separate terminal font size, and ⌘K palette access even from the terminal.
- **Composer:** Markdown highlighting with list continuation and ordered-list
  renumbering, file drops anywhere in the session column, transcript
  annotations (⌘L or ⌥-click), an empty-prompt Continue for interrupted
  turns, and per-row queued follow-ups.
- **Navigation:** Big Picture (⌘0), ⌘1–9 sidebar jumps with hold-to-reveal
  chips, Ctrl+Tab task switching with status glyphs, a ⌘N project switcher in
  New Task drafts, ⌘⌥-arrow turn navigation, ⌘D to the next unread
  completion, ⌘⇧D mark-unread-and-next, and optional three-finger swipe.
- **Transcript:** clipped long prompts with Show more, shift-click selection
  extension, changed-file cards that open files and preview diffs on hover,
  and "Annotation N" citation resolution.
- **Customization:** sixteen new theme palettes (Gruvbox, Everforest,
  Kanagawa, Zenburn, Poimandres, GitHub light/dark, Dracula, Rosé Pine
  Dawn/Moon, Kansō Zen/Pearl, Warm Burnout light/dark), split light/dark
  theme slots with a
  match-system toggle, UI and code font pickers, a completion-sound picker
  with audition and volume, and a sidebar-transparency toggle.
- **Providers and daemon:** Devin CLI support, install/sign-in actions on the
  Providers page, the `goddard-agent` agent-tools setting, and daemon resilience
  (unresponsive-daemon detection, provider-process guarding, automatic
  remote-session reconnect).
- **Polish:** menus and dialogs that reveal from their anchor, shortcut hints
  on tooltips and menu items, an in-app shortcut cheatsheet, smoothed
  (squircle) corners throughout, and ⌘H to hide on macOS.

## Getting started

### Install

- **macOS:** download the signed `.dmg` from [goddardai.org](https://goddardai.org). It
  updates itself.
- **Linux:** `curl -fsSL https://goddardai.org/install.sh | sh` — installs into
  `~/.local` without root, adds an applications-menu entry, and keeps itself
  updated. Requires glibc 2.35+ (Ubuntu 22.04, Debian 12, Fedora 36 or newer),
  a working Vulkan or OpenGL driver, and x86_64 or aarch64.
- **Windows:** run `Goddard-<version>-<arch>-Setup.exe` from the latest
  release, or unpack the portable `.zip`. Per-user install, no admin rights
  needed. Requires Windows 10 1809+, Direct3D 11 (feature level 11_0), x86_64
  or aarch64.

### Set up a provider

Goddard detects installed agent CLIs automatically — including ones installed
through PATH managers like nvm, fnm, bun, cargo, or scoop. Install and sign in
with the agent's own CLI first; **Settings → Providers** shows what was
detected, lets you enable or disable each provider for new tasks, and offers a
manual binary path if detection missed. Each provider row also has setup help
(install command, sign-in command, docs link) that can run in a terminal tab
right in the app.

## Supported agents

Thirteen providers are wired in behind one shared interface:

| Provider | CLI command | Mid-turn steering | Interactive approvals | Rewind & branch | Model picker | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| Claude Code | `claude` | yes | yes | yes | fixed catalog | |
| Codex CLI | `codex` | yes | yes | yes | yes | Goals, `/fast`, Computer Use |
| Cursor CLI | `cursor-agent` | yes | yes | yes | yes | |
| Devin CLI | `devin` | yes (transport) | yes | no | yes | |
| DeepSeek Harness | `dsh` | — | — | — | yes | Agent presets: Standard, Code, Minimal, Creator |
| Fx | `fx` | no | yes | no | yes | Vercel's agent |
| Grok Build | `grok` | yes | yes | yes | yes + effort | |
| Kimi Code | `kimi` | yes (transport) | yes | no | yes | |
| Amp | `amp` | yes | no | yes | no (modes) | Full access only |
| OpenCode | `opencode` | yes | yes | yes | yes + effort | |
| OpenCode 2 | `opencode2` | yes | yes | yes | yes + effort | |
| Pi | `pi` | yes | no | yes | yes | Full access only (Pi has no permission system) |
| Oh My Pi | `omp` | yes | runs `--yolo` | yes | yes | Full access only |

What that means in practice: whatever is running underneath, you get the same
transcript, the same permission prompts, the same model picker, and the same
rewind/fork controls — with per-provider gaps surfaced honestly instead of
silently emulated.

### Resuming sessions from the terminal

Conversations you started directly in an agent CLI are not stranded. The
command palette's **Resume…** (or the `/resume` slash command) lists sessions
each provider has on disk and imports them into Goddard with full session
continuity, across every provider.

## Core concepts

### Projects

A project is a folder on your machine (or on the daemon host, for remote
setups). You open a project once, and every task inside it shares that folder.
You can also start a task **without a project** — Goddard creates a scratch
workspace under `~/.goddard/projects/<date>/<slug>`.

### Tasks (sessions)

A task is one agent conversation bound to a workspace. Tasks are independent:
each has its own provider, model, access mode, transcript, working directory,
terminals, and Git state. A task keeps streaming while you look at another
one — nothing pauses when you switch.

Task titles come from the provider itself wherever possible (every agent but
Codex already names its own sessions; Goddard generates one for Codex on a
cheap model). Until a title lands, the first few words of your prompt act as a
placeholder. You can always rename a task inline — a name you type is never
overwritten.

Tasks can be **pinned**, **marked unread**, **archived**, or **deleted** from
the sidebar. Archiving a task that sits in a worktree snapshots the checkout
first; unarchiving restores it. A projectless task's workspace is zipped into
`~/.goddard/archives/<date>/<slug>.zip` and removed the same way. Archived chats
are hidden from the sidebar and search, browsable in Settings → Archived, and
permanently removed after 30 days.

### Worktrees

When creating a task you choose where it works: the project's ordinary
checkout (**Local**) or a **new Git worktree** created from the default branch
or any ref. Worktree tasks get isolated working directories — named after
random adjective-noun pairs and placed beside the repository — so parallel
tasks never collide over the same files. Goddard remembers your last
workspace mode and worktree branch per project, warns in the new-task area
when a checkout trails its upstream (with a one-click sync), and recreates a
worktree that went missing on submit. An existing local task can be **moved
to a worktree** later, carrying its uncommitted state with it — and the agent
is told its working directory changed.

### The window layout

- **Sidebar (left):** projects and tasks, grouped by project or date, ordered
  by last-updated or last-created, with collapsible sections. Task rows show
  live status (working, waiting for input, waiting for background tasks,
  failed, unread), dirty/unpushed Git markers, a worktree icon when the task
  runs in one, linked pull-request state with check/review status, and
  shortcut tags (⌘1–⌘9) when the modifier is held. Hover a row to archive;
  double-click its title to rename.
- **Transcript (center):** the conversation — your messages, agent replies
  rendered as Markdown, and every tool call as a normalized activity row
  (commands, file edits, reads, searches, plans, web browsing, subagents).
- **Composer (bottom):** where you type prompts.
- **Right panel:** tabbed surfaces — Review (diff), Files, Terminal, Browser —
  plus panels for background work and environment info. Any tab can maximize
  to fill the window, and Markdown files get a fullscreen reading mode.
- **Big Picture (⌘0):** an overlay of the tasks most worth a glance — waiting
  on input, unread completions, running work — each with a live tail of its
  transcript and a docked composer you can reply from without leaving the
  overlay.
- An **unseen-completion bell** in the top bar collects finished turns you
  haven't looked at yet.

## Working with agents

### The composer

- **Send** with Enter; newline with Shift+Enter.
- **Queue a follow-up** while the agent is working — queued messages appear as
  cards above the composer with per-message actions (steer it into the running
  turn, remove it), and start a new turn when the current one settles. With an
  empty composer, Cmd/Ctrl+Enter steers the oldest queued follow-up.
- **Steer** with Cmd/Ctrl+Enter: inject the message into the *running* turn
  when the provider supports it. If steering is refused or unsupported, the
  message falls back to the follow-up queue automatically.
- **Edit and resend** earlier messages.
- **Structured questions** — when an agent needs input, the composer turns
  into a multi-question form with progress ("1 of 3"), Back/Next navigation,
  preset choices, and a free-text "other" answer, instead of making you reply
  in prose.
- **@-mention files** — fuzzy-matched against the workspace file index — to
  pin them into context.
- **Slash commands** — provider-native commands and skills the agent CLI
  advertises (invoked with the provider's own syntax), plus Goddard's own
  (`/resume`, `/goal`, `/fast`).
- **Attachments** — drop or paste files, folders, and images anywhere in the
  session column (not just the composer); they're uploaded to the daemon and
  shown as chips.
- **Drafts** — unsent composer text (and its attachments) is preserved per
  task, so switching tasks never loses a half-typed prompt. Typing with
  nothing focused lands in the composer automatically.
- **Markdown-aware editing** — the composer highlights Markdown, continues
  lists on Enter, renumbers ordered lists, and lets Backspace delete an empty
  list item.
- **Continue** — if a turn fails or was interrupted, an empty composer offers
  to continue the session without typing a prompt.
- **Annotations** — select text inside an agent message and choose "Add to
  chat" (⌘L), or ⌥-click a transcript line, to pin a highlighted comment on
  the passage. The composer shows an "N annotations" chip; your next message
  carries the quoted passages and comments to the agent, and the sent bubble
  echoes them. References like "Annotation 1" inside a comment resolve into
  hover citations. A draft made only of annotations still sends.

### Models and effort

The model picker lists every model the provider reports, with a **Favorites**
tab for starred models — and it remembers which tab you used last. Where the
provider exposes them, you also get:

- **Reasoning effort** — None through Max, including provider-specific tiers
  like Ultracode.
- **Service tier** — e.g. Codex's fast mode (`/fast` toggles it), Amp's fast
  tier.
- **Context window** selection — e.g. Claude's 200K/1M options.
- **DeepSeek agent presets** — Standard, Code, Minimal, Creator, and custom
  presets.

Model, effort, and tier changes apply to the running session in place for most
providers; the ones that can't switch mid-session restart their provider
process transparently while keeping the transcript.

### Access modes and permissions

Four modes, switchable per task and remembered between launches:

| Mode | Behavior |
| --- | --- |
| Supervised | Ask before commands and file changes |
| Auto-accept edits | Auto-approve edits, ask before other actions |
| Auto | An AI reviewer approves routine actions; risky ones still ask |
| Full access | Allow commands and edits without prompts |

When an agent asks for permission, the prompt offers **Allow once**, **Allow
for session**, or **Always allow** (where the provider supports each). Pi and
Oh My Pi run in full access only — Pi has no permission system at all, and
Goddard runs Oh My Pi with `--yolo`. Changing the access mode restarts the
provider session, deliberately: tightening or loosening what a running agent
may touch gets a fresh thread rather than mutating policy mid-flight.

### Rewind, revert, and fork

Every turn in a Git-backed task leaves a conversation-aware checkpoint:

- **Revert to here** rolls the conversation *and* the working tree back to
  before that turn — the provider's own rollback where it exists (Codex
  `thread/rollback`, Pi `fork`), with the Git checkpoint restoring files.
- **Fork task** copies the session into a new task at any response, via the
  provider's native session fork where it exists.
- Both are per-provider — the table above marks Fx, Kimi, and Devin CLI as not
  supporting them yet.

### Stopping and background work

- **Stop** (or Escape, pressed twice) interrupts the turn. For providers with
  a real interrupt the session stays alive; for Codex and Amp, stopping ends
  the provider process and the next prompt resumes the native thread.
- **Background work**: long-running commands the agent detaches and subagents
  it spawns appear in the Background panel with live output, status, and stop
  controls. A turn that finishes while background work is still going shows
  "Waiting for background tasks" and can re-enter when that work settles
  (e.g. Claude background tasks stream their output live).

### Goals (Codex)

`/goal` sets a persistent objective the Codex thread keeps pursuing — before
or after the first message. Its autonomous progress streams into the
transcript, a status chip shows live budget or elapsed time, and a dialog lets
you edit, pause, resume, or clear the goal. Goal status covers active, paused,
stalled, usage-limited, budget-exhausted, and complete.

## The transcript

- Agent messages render as full Markdown: headings, lists, tables, code blocks
  with syntax highlighting, and LaTeX math (inline and block, with a
  copy-expression action; can be switched to raw source in settings).
- User messages render as Markdown too, with bare URLs linkified.
- **Tool activity** is normalized across providers into readable rows —
  "Ran `npm test`", "Edited src/app.rs" — that expand into full arguments and
  output, including inline diffs for file edits. Long groups collapse as the
  turn moves on.
- **Reasoning** streams into collapsible "Thinking" sections with elapsed
  time.
- **Find in page** (Cmd/Ctrl+F) searches the whole transcript with match-case,
  whole-word, and regex toggles.
- Long user prompts are clipped with a **Show more** control instead of
  scrolling; Shift-click extends a text selection.
- Each response keeps a changed-files summary and a **Review** action that
  opens its diff — files in the summary open directly, and hovering one
  previews its diff inline.
- A **navigation rail** alongside the transcript marks each turn — hover for
  a preview of its prompt, click to jump; ⌘⌥↑/↓ step between them.
- Every response shows its working time ("Worked for 3m") and a copy action;
  selected text copies with ⌘C, and code blocks, commands, and file paths
  have their own copy controls.
- Every user message supports edit, and every response supports fork/revert —
  the transcript is the control surface, not just a log.

## Git integration

- **Git panel:** changed files with stage/unstage, commit list, unpushed
  markers, push, and **Sync changes** — `git pull --rebase` by default, or
  merge if you prefer (a setting). Rebase/merge conflicts surface with an
  **Abort**, **Merge instead**, or **Resolve in chat** action that hands the
  conflict to the agent.
- **Commit dialog:** Commit, Commit and push, or Push, with an
  include-unstaged toggle. Write your own subject or leave it empty and
  Goddard generates one — by running the session's own agent CLI once,
  headlessly, pinned to that provider's cheapest tier (Claude → Haiku, Codex →
  gpt-5.6-luna) so generation stays fast and nearly free.
- **Branch picker:** search, switch, and create branches, with "checked out in
  another worktree" marked.
- **Review surface:** diff the last turn, any turn, uncommitted/staged/
  unstaged/committed changes, a branch, or a specific commit — with per-file
  trees, expandable context lines, and a filter box.
- **Compare branch** and commit/push shortcuts live in the task's environment
  panel alongside its Task ID and agent CLI thread ID (both copyable).

## GitHub integration

For projects backed by a GitHub repo, a **GitHub** entry in the sidebar opens
a pull-request and issue browser in place of the transcript — state filters,
search, full PR/issue detail — powered by the `gh` CLI on the daemon host.
From a PR you can **start a task** on it or ask an agent to **fix failing
checks**. Sidebar task rows show linked PR state (open/draft/merged/closed)
with check and review status.

## Files, terminal, and browser

### Files surface

A workspace file tree with a reading/editing pane: syntax highlighting,
Markdown source/preview toggle, save with Cmd/Ctrl+S, find and replace
(Cmd/Ctrl+F inside the editor, with case/whole-word/regex), go to line
(Ctrl+G), and "Open on
GitHub" for tracked files. **Cmd+P** opens a fuzzy file finder over the same
index. On macOS, an **Open in…** control opens the project folder in an
external app and remembers your choice — VS Code, Cursor, Zed, Devin, Finder,
Terminal, Termy (via its new-tab deeplink), iTerm2, Kitty, Ghostty, Warp,
Xcode, and Android Studio are detected automatically.

### Terminal

Real PTY terminals in right-panel tabs, scoped to the task's workspace —
multiple per task, plus global terminals in the sidebar's Terminals group
(pinnable). macOS/Linux run your login shell; Windows prefers PowerShell 7,
falls back to Windows PowerShell, then `COMSPEC`. A shell-integration script
sourced into each PTY reports the live working directory and per-command
boundaries, so sidebar rows are named after the repo (falling back to the
shell), track `cd`, show last-activity times and command status icons, and
close themselves when the shell exits. URLs in terminal output are clickable
(OSC8 hyperlinks and plain-text links). Scrollback clear, terminal font
sizing, and copy/paste that doesn't steal Ctrl+C from the shell
(Ctrl+Shift+C/V on Windows).

### Embedded browser

A browser surface in the right panel — WebKit on macOS, WebView2 on Windows —
with an address bar, navigation, reload/stop, devtools, downloads, and "open
in default browser". When the terminal detects a dev server (a `localhost`
URL appearing in output), a "**localhost is live** — Open" toast offers to open
it in a browser tab in one click.

## Navigation and keyboard control

Goddard is designed to be driven without a mouse:

- **Command palette (⌘K / Ctrl+K):** fuzzy search across tasks, commands,
  settings pages, providers, projects, scripts, and CLI sessions to resume —
  plus **full chat-history search**: the query runs against every stored
  message in every task and returns matching snippets (archived chats are
  excluded; they're searched separately in Settings → Archived).
- **Task switcher (Ctrl+Tab / Ctrl+Shift+Tab):** recently-used tasks in an
  overlay; releasing the modifier commits.
- **Project switcher:** same pattern, for recent projects.
- **Jump to task:** hold the primary modifier and press 1–9 for the sidebar's
  first tasks.
- **Turn navigation:** jump to previous/next turn, next unread completion,
  or mark-unread-and-go-to-next.
- **History navigation:** back/forward across where you've been, plus
  three-finger trackpad swipe between tasks on macOS (optional setting).
- **Find in transcript:** Cmd/Ctrl+F.
- Shortcuts are surfaced where you need them: menus, tooltips on composer
  selectors and sidebar actions, and the command palette all show their
  chords. The full cheatsheet lives in the app — the keyboard button beside
  the sidebar's settings icon opens a shortcuts dialog resolved from the live
  keymap.

## Customization

**Settings → Appearance:**

- Match system appearance, or pick light/dark independently.
- Separate light and dark theme palettes: Default, Gruvbox, Everforest,
  Kanagawa, Zenburn, Poimandres, GitHub, Dracula, Rosé Pine (Dawn/Moon),
  Kanso (Zen/Pearl), and Warm Burnout.
- UI font and code font (any installed family), with independent sizes for UI
  text, code/diff/editor text, and the terminal.
- Sidebar transparency (vibrancy) on or off.
- Interface language: System, English, 日本語, 简体中文.
- Window size, position, and display are restored across launches.

Accessibility: every control is keyboard-operable with visible focus
treatments, the system reduce-motion setting is honored, and status is never
carried by color alone. One honest limitation: GPUI does not yet expose a
screen-reader tree, so VoiceOver and equivalents are not supported.

**Settings → General:** automatic updates, anonymous usage-data sharing
(off by default — and prompts, responses, project names, and file paths are
never collected even when on), LaTeX math rendering, Markdown preview,
open-at-last-prompt, sync-with-merge, three-finger swipe navigation, sidebar
shortcut tags, and a completion sound (with volume) that plays when a task
you're not viewing finishes.

## Skills

A **Skills library** in settings lists every skill installed for any provider
ecosystem — user-level and project-level — as a master/detail browser with
contents, supporting files, allowed tools, and update times. Skills can be
enabled, disabled, revealed, or deleted, and are invoked in the composer as
`/name`. Duplicate names resolve by specificity.

## Custom commands

Settings → Commands defines named shell scripts that appear in the command
palette (including a "New custom command" palette action). Each runs in a new
terminal tab in the current task's directory, inside an interactive shell (so
aliases, pipes, and interactive programs work), with an optional icon, a
custom shell, and a close-on-success option. Runs report through a toast that
mirrors the command's output tail. Agents can add commands too (a daemon
setting, on by default; agent-added commands are badged).

## Usage and cost tracking

- A **context-window gauge** under the composer shows live occupancy for the
  current session and opens a panel with account rate-limit lanes (Claude plan
  limits via the OAuth endpoint, OpenCode Go's usage endpoint, Codex's own
  rate-limit notifications) with reset countdowns.
- **Settings → Usage** is a full dashboard: daily and monthly cost/token
  charts, per-project and per-model breakdowns, cache savings, cost-quality
  indicators, and custom date windows — computed by scanning provider
  transcripts on the daemon.

## Notifications

When a task you're not viewing finishes its turn, Goddard can play a
completion sound — the picker lets you audition each option (including a
Retro chime) and set its volume — and posts a native system notification;
clicking it opens the task. The sound stays quiet while a follow-up is still
queued, since the task isn't done waiting on you. The sidebar marks unseen
completions as unread until you look at them, and those unread stamps survive
restarts.

## Architecture: daemon, web, and mobile

Goddard Desktop is an RPC client of **`goddard-daemon`**, a standalone process
that owns task data, transcripts, attachments, provider processes, and all
filesystem/Git operations. The desktop keeps only presentation state. That
split is what makes the other clients possible:

- **Expose the daemon** (Settings → Daemon): the managed daemon can listen on
  a fixed port beyond loopback with an explicit allowlist of browser origins
  and a stable authentication token. The token grants full control — treat it
  like a password, and use `wss://` through a trusted TLS proxy outside a
  private network.
- **Goddard Web** is a browser client that connects to an exposed daemon over
  that authenticated WebSocket — same tasks, transcripts, diffs, and
  permissions, rendered in the browser. It includes a **daemon-host file
  picker**, so you can browse the remote machine's folders to open a project
  or attach files to a prompt.
- **Goddard Mobile** (iOS/Android, Expo) connects to one or more remote
  daemons the same way; tokens are stored in the device keychain.
- **Agent tools** (daemon setting): sessions can get a session-scoped
  `goddard-agent` command that lets one agent create tasks and send messages to
  other tasks — agent-to-agent delegation, marked in the target transcript.
- The desktop can also **connect to an externally managed daemon** (headless
  host, VM, container): files, diffs, Git, skills, usage, and attachments all
  work over RPC. Two things still need a local daemon on the desktop: picking
  a project folder on the remote host (the web client's daemon file picker
  covers this case) and PTY terminals.
- The connection is built to fail well: an unresponsive daemon is detected and
  retried with backoff, remote sessions reconnect automatically after
  interruptions, provider processes are guarded against daemon death, and
  daemon failures surface in the UI where they would otherwise cause silent
  damage.

## Privacy and data storage

- **Local by default.** Projects, conversations, settings, and attachments
  live on your machine. There is no Goddard account and no required remote
  service.
- On macOS/Linux, app state lives under `~/.goddard/` (`app.json` for settings;
  projectless workspaces under `~/.goddard/projects/`, their zipped archives
  under `~/.goddard/archives/`); daemon provider and Computer Use settings in
  `~/.goddard/settings.json`. On Windows, task data is
  `%LOCALAPPDATA%\Goddard\app.db`, blobs alongside it, settings in
  `%USERPROFILE%\.goddard\app.json`.
- Optional anonymous analytics cover feature usage and reliability only — the
  toggle is off by default and prompts, responses, project names, and file
  paths are never included.
- Goddard holds **no provider API keys**. Authentication is whatever each
  agent CLI does for itself; even commit-message generation and title
  generation go through the CLI binary rather than a Goddard-held key.

## Updates

Goddard checks for updates once per launch (disable in Settings → General) and
shows available releases in the sidebar footer. Every platform verifies the
same Ed25519/EdDSA release signature before installing:

- **macOS:** Sparkle in-app updates with release notes.
- **Linux:** the updater validates the staged install, swaps the prefix, and
  relaunches — with automatic rollback to the previous version if the new
  build exits before opening a window.
- **Windows:** downloads and runs the verified installer, which replaces the
  app in place (including portable installs).

## Computer Use (experimental)

Debug builds can give supported providers (Codex, OpenCode, OpenCode 2, Grok,
Pi) desktop control through the bundled Cua Driver SDK: agents get `js`/`js_reset`
REPL tools that can observe windows, move the cursor, click, and type — with a
live preview in the app. On macOS it requires Screen Recording and
Accessibility grants to a separate, isolated helper process; grants can be
scoped per-task or always-allowed per app. It is a development-only feature —
release builds clamp the flag before any driver starts.

## Keyboard shortcut reference

Highlights (macOS chords; on Linux/Windows the primary modifier is Ctrl). The
in-app shortcuts dialog — resolved from the live keymap — is authoritative.

| Action | Shortcut |
| --- | --- |
| New task | ⌘N |
| New project | ⌘O |
| Command palette | ⌘K |
| File finder | ⌘P |
| Big Picture | ⌘0 |
| Jump to task 1–9 | ⌘1–⌘9 (hold ⌘ to see the chips) |
| Navigate back / forward | ⌘[ / ⌘] |
| Previous / next turn | ⌘⌥↑ / ⌘⌥↓ |
| Next unread completion (idle tasks once drained) | ⌘D or Ctrl+` |
| Mark unread, go to next idle task | ⌘⇧D |
| Task switcher | Ctrl+Tab / Ctrl+Shift+Tab |
| Project switcher (in a New Task draft) | hold ⌘, tap N |
| Toggle sidebar / right panel | ⌘B / ⌘⌥B |
| Focus composer | ⌘L (Add to chat when transcript text is selected) |
| Focus terminal / terminals group | ⌘J / ⌘T |
| Model / branch / mode picker | ⌘/ / ⌘⇧B / ⌘. |
| Workspace picker / usage panel | ⌘⇧T / ⌘U |
| Run project script | ⌘R |
| Find / find-and-replace | ⌘F / ⌘⌥F, then ⌘G / ⌘⇧G |
| Go to line (file viewer) | Ctrl+G |
| Open detected localhost URL | ⌘⌥O (⌘⌥⇧O opens a new browser tab) |
| Stop turn | Esc, Esc again to confirm; ⌥Esc stops immediately |
| Archive / pin task | ⌘⇧A / ⌘⌥P |
| Copy selection / working directory | ⌘C / ⌘⇧C |
| Save file | ⌘S |
| Send / newline / steer | Enter / ⇧Enter / ⌘Enter |
| Font size (follows focus: UI, code, terminal) | ⌘= / ⌘− |
| Clear terminal scrollback | ⇧⌘K |
| Browser: address bar, back/forward, reload, devtools | ⌘L, ⌘[/⌘], ⌘R (⌘⇧R hard), ⌘⌥I |
| Settings | ⌘, |
| FPS counter | ⌘⌥⇧F |
| Close window / quit | ⌘W / ⌘Q |
| Hide app / hide others (macOS) | ⌘H / ⌘⌥H |

## Platform differences

| Feature | macOS | Linux | Windows |
| --- | --- | --- | --- |
| Agent sessions, projects, transcripts, diffs, Git, skills, usage | yes | yes | yes |
| Embedded browser | WebKit | — | WebView2 (no load-progress bar; devtools open-only; no pen/touch/file-drop yet) |
| Integrated terminal | login shell | login shell | PowerShell 7 → Windows PowerShell → COMSPEC |
| In-app updates | Sparkle | signed, with rollback | signed installer |
| Computer Use | debug builds | debug builds | debug builds |
| Remote-daemon terminal for browser clients | yes | yes | not yet |
| Three-finger swipe navigation | yes | — | — |

## Troubleshooting

**A provider shows as not installed.** Run the CLI by name in a fresh terminal.
If the shell can't find it either, the install never put a shim on `PATH`. If
the shell finds it but Goddard doesn't, set the binary path in **Settings →
Providers**. Detection already knows about version managers (nvm, fnm) and
per-user prefixes (`%APPDATA%\npm`, `~/.bun/bin`, `~/.cargo/bin`, scoop,
WindowsApps).

**The window opens black, or the app exits at startup (Windows).** Goddard
needs a working Direct3D 11 device — update the GPU driver; in a VM, enable 3D
acceleration.

**The app dies on its first frame in a Linux VM.** Software Vulkan/GL
rasterizers (lavapipe, llvmpipe) can crash compiling shaders — a driver bug,
not Goddard's. Give the guest a real GL driver (e.g. virtio-gpu-gl on UTM), or
set `VK_DRIVER_FILES=/nonexistent.json` to force the GL path.

**Git-backed features do nothing.** Goddard shells out to `git` — make sure
`git --version` works in a new terminal (install Git for Windows on Windows).

**Updates never arrive.** The updater fetches `releases.goddardai.org` (via
`curl.exe` in System32 on Windows); a proxy or filter blocking that host
blocks updates. **Check for Updates…** in the app menu reports the reason.
Downloading and running the installer manually is always equivalent.

**SmartScreen warns on first launch (Windows).** Expected when the release
isn't code-signed — choose **More info → Run anyway**.

## Uninstalling

- **macOS:** move Goddard to the Trash. Settings, tasks, and workspaces live in
  `~/.goddard` — delete it to remove them too.
- **Linux:** `curl -fsSL https://goddardai.org/install.sh | sh -s -- --uninstall`
  removes `~/.local/goddard.app`, the symlink, and the desktop entry. `~/.goddard`
  stays; delete it to remove projects and settings.
- **Windows:** uninstall from Settings → Apps, or delete the portable folder.
  Task data is `%LOCALAPPDATA%\Goddard`, settings `%USERPROFILE%\.goddard`.

## FAQ

**Does Goddard replace Claude Code / Codex / etc.?**
No — it drives them. You need at least one agent CLI installed and signed in.
Goddard replaces each CLI's terminal interface with a shared native one.

**Is it native or another Electron shell?**
Native down to the frame — Rust on GPUI, the GPU-accelerated framework behind
Zed. No Electron, instant launch, and scrolling that holds up on a 120 Hz
display through long transcripts.

**How is it different from an editor with AI built in?**
An editor with AI ties you to one provider inside one editor. Goddard sits
alongside your existing setup and runs the agents you already subscribe to —
you pick the best tool per task, not the one bundled in.

**Do my agent's own config still work — MCP servers, hooks, AGENTS.md?**
Yes. Goddard launches the real CLI through its native session protocol, so the
agent loads its own configuration, MCP servers, hooks, skills, and project
instructions exactly as if you'd run it in a terminal.

**Do I pay Goddard anything?**
No. It's free (~80 MB download) and GPL-licensed. "Bring your own
subscription" is literal — there's no bundled plan or markup, and your agent
provider bills exactly as if you used the CLI directly. Your existing plans,
rate limits, and API keys apply unchanged.

**Where do my conversations go?**
Nowhere. Everything is stored locally (or on a daemon host you control).
There's no Goddard account and no sync service.

**Can it run agents while I'm away?**
Agents run as local processes under the daemon — they keep working while the
window is closed to another task, and the daemon keeps sessions resumable, but
they still need your machine (or your daemon host) to be on.

**If I quit Goddard, do I lose my sessions?**
No. Tasks, transcripts, drafts, and queued follow-ups are persisted locally
and reopen where you left them. Idle provider processes are reaped after
about 30 minutes, but that's invisible — the next prompt resumes the native
session.

**Can I use it on a server/headless machine?**
Yes: run `goddard-daemon` on the host, expose it with `--allow-non-loopback`, an
origin allowlist, and a token, then connect with Goddard Web, the mobile app,
or a desktop pointed at the external daemon.

**What can agents do on my machine?**
Agents run under your user account, and you pick the access mode per task —
from approving every action to letting the agent work unattended. Goddard
maps your choice onto each provider's own permission system rather than
inventing a second one.

**Can several agents work in parallel?**
Yes — that's the core design. Independent tasks run simultaneously, each in
its own workspace or worktree, and keep streaming in the background.

**Can one agent talk to another?**
With the daemon's Agent Tools setting enabled, sessions get a `goddard-agent`
command that can create tasks and send messages to other tasks.

**Can I import a session I started in the terminal?**
Yes — command palette → Resume… lists resumable CLI sessions per provider.

**Can I search everything I've ever asked an agent?**
Yes. The command palette searches message content across all tasks, not just
titles — type a few words you remember and matching messages come back as
snippets. Cmd/Ctrl+F does the same thing within the open transcript.

**Can I undo what the agent did?**
Revert to Here rolls back both the conversation and the Git working tree to
before a turn; Fork copies a session at any point. Both use provider-native
mechanisms where they exist.

**Does it work offline?**
The app does; your agent's model calls obviously don't. Everything except the
provider round-trip is local.

**Can I export or back up my data?**
There's no built-in export tool — but there's also no lock-in. Everything is
ordinary files you can browse, copy, or back up from the system file manager:
`~/.goddard` on macOS/Linux, `%LOCALAPPDATA%\Goddard` plus `%USERPROFILE%\.goddard`
on Windows. Provider transcripts also remain in each CLI's own session store
(`~/.claude/projects`, Codex threads, etc.).

**Can I open multiple windows?**
No — Goddard is a single-window app by design. Parallel work lives in
sidebar tasks, ⌘1–9 jumps, the Ctrl+Tab switcher, and Big Picture (⌘0)
rather than separate windows. Window size, position, and display are restored
across launches.

**Can I customize the keybindings?**
Not yet — the keymap is fixed (and compiled), so there's no rebinding UI or
config file today. Themes, fonts, and sizes are the customization surface.

**Is there telemetry?**
Optional, anonymous, off by default — feature-usage and reliability data only,
never prompt/response content, project names, or file paths.

**Which permission mode should I use?**
Supervised if you want to approve every action; Auto-accept edits to let file
edits through while still approving other actions; Auto to let an AI reviewer
handle routine approvals and only ask about risky ones; Full access for
hands-off runs (required for Pi, Oh My Pi, and Amp).

**How do I report a bug or contribute?**
The project is open source (GPL-3.0) on GitHub — open an issue there, or join
the Discord linked from the site's menu. See CONTRIBUTING.md for the
development workflow.
