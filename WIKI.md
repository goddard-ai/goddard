# Goddard Wiki

Everything a user might want to know about Goddard: what it is, what it does,
how it works, and whether it fits the way you want to work.

## What is Goddard?

Goddard is a fast, native desktop app for working with local coding agents.
Instead of living inside one agent's terminal UI, you run **Claude Code, Codex,
Cursor, OpenCode, and nine other agent CLIs** side by side from one interface —
each task gets a real transcript, a diff view, a terminal, a file browser, and
Git tooling, while the agent CLI does the actual work underneath.

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
| Where is my data | Locally on your machine — no Goddard cloud service |
| Languages | English, Japanese, Simplified Chinese |
| Updates | Automatic, cryptographically signed, with rollback on Linux |

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

## Getting started

### Install

- **macOS:** download the signed `.dmg` from [waku.sh](https://waku.sh). It
  updates itself.
- **Linux:** `curl -fsSL https://waku.sh/install.sh | sh` — installs into
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
workspace under `~/.waku/projects/<date>/<slug>`.

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
first; unarchiving restores it. Archived chats are hidden from the sidebar and
search, browsable in Settings → Archived, and permanently removed after 30
days.

### Worktrees

When creating a task you choose where it works: the project's ordinary
checkout (**Local**) or a **new Git worktree** created from the default branch
or any ref. Worktree tasks get isolated working directories, so parallel tasks
never collide over the same files. An existing local task can be **moved to a
worktree** later, carrying its uncommitted state with it.

### The window layout

- **Sidebar (left):** projects and tasks, grouped by project or date, ordered
  by last-updated or last-created, with collapsible sections. Task rows show
  live status (working, waiting for input, waiting for background tasks,
  failed, unread), linked pull-request state with check/review status, and
  shortcut tags (⌘1–⌘9) when the modifier is held.
- **Transcript (center):** the conversation — your messages, agent replies
  rendered as Markdown, and every tool call as a normalized activity row
  (commands, file edits, reads, searches, plans, web browsing, subagents).
- **Composer (bottom):** where you type prompts.
- **Right panel:** tabbed surfaces — Review (diff), Files, Terminal, Browser —
  plus panels for background work and environment info.
- **Big Picture (⌘0):** an overlay of the tasks most worth a glance — waiting
  on input, unread completions, running work — each with a live tail of its
  transcript and a docked composer you can reply from without leaving the
  overlay.

## Working with agents

### The composer

- **Send** with Enter; newline with Shift+Enter.
- **Queue a follow-up** while the agent is working — queued messages appear as
  cards above the composer and start a new turn when the current one settles.
- **Steer** with Cmd/Ctrl+Enter: inject the message into the *running* turn
  when the provider supports it. If steering is refused or unsupported, the
  message falls back to the follow-up queue automatically.
- **Edit and resend** earlier messages, and answer an agent's questions
  directly in the composer.
- **@-mention files** — fuzzy-matched against the workspace file index — to
  pin them into context.
- **Slash commands** — provider-native commands and skills the agent CLI
  advertises (invoked with the provider's own syntax), plus Goddard's own
  (`/resume`, `/goal`, `/fast`).
- **Attachments** — drop or paste files, folders, and images onto the
  composer; they're uploaded to the daemon and shown as chips.
- **Drafts** — unsent composer text (and its attachments) is preserved per
  task, so switching tasks never loses a half-typed prompt.
- **Annotations** — select text inside an agent message and choose "Add to
  chat" to pin a highlighted comment on the passage. The composer shows an
  "N annotations" chip; your next message carries the quoted passages and
  comments to the agent.

### Models and effort

The model picker lists every model the provider reports, with a **Favorites**
tab for starred models. Where the provider exposes them, you also get:

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
- Each response keeps a changed-files summary and a **Review** action that
  opens its diff.
- Every user message supports edit, and every response supports fork/revert —
  the transcript is the control surface, not just a log.

## Git integration

- **Git panel:** changed files with stage/unstage, commit list, unpushed
  markers, push, and **Sync changes** — `git pull --rebase` by default, or
  merge if you prefer (a setting). Rebase/merge conflicts surface with an
  **Abort**, **Merge instead**, or **Resolve in chat** action that hands the
  conflict to the agent.
- **Commit dialog:** write your own subject or leave it empty and Goddard
  generates one — by running the session's own agent CLI once, headlessly,
  pinned to that provider's cheapest tier (Claude → Haiku, Codex → gpt-5.6-luna)
  so generation stays fast and nearly free.
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
(Cmd/Ctrl+F inside the editor, with case/whole-word/regex), and "Open on
GitHub" for tracked files. **Cmd+P** opens a fuzzy file finder over the same
index.

### Terminal

Real PTY terminals in right-panel tabs, scoped to the task's workspace —
multiple per task, plus global terminals in the sidebar's Terminals group
(pinnable). macOS/Linux run your login shell; Windows prefers PowerShell 7,
falls back to Windows PowerShell, then `COMSPEC`. Scrollback clear, terminal
font sizing, and copy/paste that doesn't steal Ctrl+C from the shell
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
  settings pages, providers, projects, scripts, and CLI sessions to resume.
- **Task switcher (Ctrl+Tab / Ctrl+Shift+Tab):** recently-used tasks in an
  overlay; releasing the modifier commits.
- **Project switcher:** same pattern, for recent projects.
- **Jump to task:** hold the primary modifier and press 1–9 for the sidebar's
  first tasks.
- **Turn navigation:** jump to previous/next turn, latest unseen completion,
  or mark-unread-and-go-to-next.
- **History navigation:** back/forward across where you've been, plus
  three-finger trackpad swipe between tasks on macOS (optional setting).
- **Find in transcript:** Cmd/Ctrl+F.
- The full cheatsheet lives in the app — the keyboard button in the sidebar
  opens a shortcuts dialog whose chords are resolved from the live keymap.

## Customization

**Settings → Appearance:**

- Match system appearance, or pick light/dark independently.
- Separate light and dark theme palettes — Default, Gruvbox, Everforest,
  Kanagawa, Zenburn, Poimandres, GitHub, Dracula, Rosé Pine, Kanso, and more.
- UI font and code font (any installed family), with independent sizes for UI
  text, code/diff/editor text, and the terminal.
- Sidebar transparency (vibrancy) on or off.
- Interface language: System, English, 日本語, 简体中文.

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
palette. Each runs in a new terminal tab in the current task's directory,
inside an interactive shell (so aliases, pipes, and interactive programs work),
with an optional icon, a custom shell, and a close-on-success option. Agents
can add commands too (a daemon setting, on by default; agent-added commands are
badged).

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
completion sound and posts a native system notification; clicking it opens the
task. The sidebar marks unseen completions as unread until you look at them.

## Architecture: daemon, web, and mobile

Goddard Desktop is an RPC client of **`waku-daemon`**, a standalone process
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
  permissions, rendered in the browser.
- **Goddard Mobile** (iOS/Android, Expo) connects to one or more remote
  daemons the same way; tokens are stored in the device keychain.
- **Agent tools** (daemon setting): sessions can get a session-scoped
  `waku-agent` command that lets one agent create tasks and send messages to
  other tasks — agent-to-agent delegation, marked in the target transcript.
- The desktop can also **connect to an externally managed daemon** (headless
  host, VM, container): files, diffs, Git, skills, usage, and attachments all
  work over RPC. The local folder picker and PTY terminal are the current
  exceptions until the protocol gains daemon-host equivalents.

## Privacy and data storage

- **Local by default.** Projects, conversations, settings, and attachments
  live on your machine. There is no Goddard account and no required remote
  service.
- On macOS/Linux, app state lives under `~/.waku/` (`app.json` for settings;
  projectless workspaces under `~/.waku/projects/`); daemon provider and
  Computer Use settings in `~/.waku/settings.json`. On Windows, task data is
  `%LOCALAPPDATA%\Goddard\app.db`, blobs alongside it, settings in
  `%USERPROFILE%\.waku\app.json`.
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
| Latest unseen completion | ⌘D or Ctrl+` |
| Mark unread, go to next unseen | ⌘⇧D |
| Task switcher | Ctrl+Tab / Ctrl+Shift+Tab |
| Project switcher (in a New Task draft) | hold ⌘, tap N |
| Toggle sidebar / right panel | ⌘B / ⌘⌥B |
| Focus composer | ⌘L (Add to chat when transcript text is selected) |
| Focus terminal / terminals group | ⌘J / ⌘T |
| Model / branch / mode picker | ⌘/ / ⌘⇧B / ⌘. |
| Workspace picker / usage panel | ⌘⇧T / ⌘U |
| Run project script | ⌘R |
| Find / find-and-replace | ⌘F / ⌘⌥F, then ⌘G / ⌘⇧G |
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

## FAQ

**Does Goddard replace Claude Code / Codex / etc.?**
No — it drives them. You need at least one agent CLI installed and signed in.
Goddard replaces each CLI's terminal interface with a shared native one.

**Do I pay Goddard anything?**
No. It's free and GPL-licensed. Your agent provider subscription is the only
cost, and it bills exactly as if you used the CLI directly.

**Where do my conversations go?**
Nowhere. Everything is stored locally (or on a daemon host you control).
There's no Goddard account and no sync service.

**Can it run agents while I'm away?**
Agents run as local processes under the daemon — they keep working while the
window is closed to another task, and the daemon keeps sessions resumable, but
they still need your machine (or your daemon host) to be on.

**Can I use it on a server/headless machine?**
Yes: run `waku-daemon` on the host, expose it with `--allow-non-loopback`, an
origin allowlist, and a token, then connect with Goddard Web, the mobile app,
or a desktop pointed at the external daemon.

**Can several agents work in parallel?**
Yes — that's the core design. Independent tasks run simultaneously, each in
its own workspace or worktree, and keep streaming in the background.

**Can one agent talk to another?**
With the daemon's Agent Tools setting enabled, sessions get a `waku-agent`
command that can create tasks and send messages to other tasks.

**Can I import a session I started in the terminal?**
Yes — command palette → Resume… lists resumable CLI sessions per provider.

**Can I undo what the agent did?**
Revert to Here rolls back both the conversation and the Git working tree to
before a turn; Fork copies a session at any point. Both use provider-native
mechanisms where they exist.

**Does it work offline?**
The app does; your agent's model calls obviously don't. Everything except the
provider round-trip is local.

**Is there telemetry?**
Optional, anonymous, off by default — feature-usage and reliability data only,
never prompt/response content, project names, or file paths.

**Which permission mode should I use?**
Supervised if you want to approve every action; Auto-accept edits to let file
edits through while still approving other actions; Auto to let an AI reviewer
handle routine approvals and only ask about risky ones; Full access for
hands-off runs (required for Pi, Oh My Pi, and Amp).

**How do I report a bug or contribute?**
The project is open source — issues and contributions go through its
repository; see CONTRIBUTING.md for the development workflow.
