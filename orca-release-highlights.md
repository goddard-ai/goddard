# Orca — Five Months of Shipping (April 15 – September 15, 2026)

*Everything that landed across v1.1.33 → v1.4.203 and Orca Mobile v0.0.1 → v0.0.48 — roughly 230 stable releases and 35 mobile builds.*

Orca is the ADE for running a fleet of parallel coding agents on your own subscriptions. Here's what the last five months added — and why leaving gets harder with every release.

---

## Every agent, one cockpit

- **The industry's widest agent roster.** First-class support for Claude Code, Codex, Grok, OpenCode, Cursor Agent, GitHub Copilot CLI, Devin CLI, Prime Agent, Droid, Amp, Trae CLI, Pi, OMP (oh-my-pi), Hermes, Command Code, Autohand Code, Ante, Antigravity, OpenClaude, and MiniMax — with new agents landing nearly every week.
- **Agent status hooks everywhere.** Per-pane, hook-reported status tells you whether each agent is *working*, *waiting on you*, or *done* — in terminal tabs, the sidebar, and the dashboard. It works over SSH and even in WSL.
- **The Agent Dashboard is now default-on.** A live board of every agent across every workspace: tinted state cards, a real filter panel, an experimental agent map, unread/finish/question indicators, and a pop-out window mode. One glance replaces ten terminal checks.
- **Multi-account everything.** Claude and Codex account switchers, inline usage meters for inactive accounts, and rate-limit tracking for Grok OAuth, MiniMax, Gemini, and OpenCode Go — so you always know which subscription has headroom.

## Native structured chat

- **A real chat UI for terminal agents.** Structured native chat for Claude (built on the Claude Agent SDK), Codex, and Grok — with per-model and effort pickers, image attachments with previews, `/clear` and `/compact`, session rewind, and drag-and-drop files onto the composer.
- **Readable transcripts, not opcode noise.** Tool rows are labeled by what the command actually did, file edits render as inline diff cards, and Claude/Codex subagent activity shows up as child rows instead of leaking protocol internals.
- **It follows you.** Native chat runs across desktop, mobile, and web, routes through the remote runtime for SSH sessions, and can be orchestrated like any other pane.

## Multi-agent orchestration

- **A real inter-agent orchestration system.** Coordinator-driven workers with durable multi-agent workflows, nested worker depth tracking (propagated across hosts), and automatic release of settled worker terminals.
- **Humans stay in the loop.** Workers blocked on a human prompt surface that state instead of silently stalling.
- **Per-worker control.** Model and effort overrides per worker, orchestration group routing for Grok/Cursor/Droid, and a standalone orchestration skill agents can load.

## Workspaces and worktrees

- **Split groups.** Full split-pane UI with tab-bar context menus, resize handles, and reliable session restore — run the agent, its dev server, and a scratch terminal side by side.
- **Workspaces that manage themselves.** Auto-rename branch and folder from the first prompt, non-blocking creation with in-tab progress, sparse checkout, multi-repo folder workspaces, per-project base paths, and `.worktreeinclude` / `worktree.sharedDirectories` support in `orca.yaml` for copying gitignored files in.
- **Sleep, don't kill.** Workspace Sleep (with recursive descendant sleep) and experimental agent hibernation freeze work without losing it.
- **A board, not a list.** Kanban workspace board with multi-select drag moves, marquee selection, search, plus pinned worktrees, localhost port labels, lineage grouping, and a space analyzer to see what's eating disk.

## Orca Mobile

- **A real companion app, not a viewer.** Android (and iOS via TestFlight) pairing by QR or paste, with Keychain-backed device tokens and deep links.
- **Drive your desktop from the couch.** Presence-based driver lock: when your phone takes a terminal, the desktop shows a "phone is driving" banner with a Take back button — two phones can't fight over one pane.
- **Real push notifications.** Agents that finish or ask questions reach your pocket even when the laptop lid is closed.
- **Near-full parity.** Structured native chat, source control, tasks, PR review, artifact viewing, quick commands, voice dictation (desktop-backed), a customizable terminal shortcut bar, and clipboard image paste into terminals.
- **Orca Relay.** A relay transport with closest-region selection means your phone reaches your desktop off-LAN — plus causal network diagnostics when it can't.

## Remote development

- **SSH workspaces are first-class.** Persisted, auto-restored remote sessions with ControlMaster multiplexing, Kerberos/GSSAPI support, host-key verification, passphrase prompts, and pane state that survives reconnects and app restarts.
- **Provisioned roots.** Create workspaces from provisioned SSH roots — point Orca at a VM recipe and get a ready-to-work remote environment.
- **Everything works remotely.** Port forwarding, drag-and-drop file upload over SSH, folder downloads, a 10MB+ streaming file preview, remote CLI agent detection, agent status over SSH, and one-click "open in VS Code Remote-SSH."
- **Windows is covered too.** WSL shell support, Git Bash, PowerShell 7+, a custom renderer-drawn title bar, and a full File/Edit/View menu bar on Windows and Linux.

## The built-in browser (and computer use)

- **A real browser inside your workspace.** Tabs, Cmd+F find-in-page, address-bar history, hard reload, persistent zoom, duplicate tab, WebAuthn account picker, and multi-profile session management.
- **Bring your logins.** Cookie import from Chrome/Chromium-family browsers including Comet and Helium, with per-profile pickers.
- **Agents can drive it.** Computer use over a CDP bridge — click, scroll, annotate. Add viewport-size emulation, in-page screenshot markup, page annotations with inline-editable comments, and agents get a browser they can actually operate.
- **Mobile emulation built in.** Android emulation via scrcpy and iOS simulator tabs, with agent control and Cmd+J access — test mobile flows without leaving the app.

## Source control and code review

- **A full source control panel.** Commit, push, pull, sync, fast-forward, abort-merge, and explicit force-push; a tree view with per-file line counts; staged discard-all; Cmd+Enter to commit; and an expandable git history graph.
- **AI where it helps.** AI-generated commit messages, AI recovery for failed pushes and commits, and AI actions for PR conflicts and failing checks.
- **PRs end to end.** Create PRs (including hosted and stacked PRs), enable auto-merge, view diffs as a hierarchical file tree with viewed-file tracking, comment reactions, mermaid diagrams in PR comments, and send review notes straight to an agent with one shortcut.
- **Every forge.** GitHub (plus Enterprise URLs), GitLab, Bitbucket, Azure DevOps, and Gitea — hosted review support across all of them.

## Tasks, issues, and automations

- **Linear, Jira, GitHub, GitLab issues — first-class.** Inline editing, issue relations, activity timelines, create-issue dialogs, Linear team/status filters and estimates, self-hosted Jira Server/DC support, and "create a workspace from this issue" everywhere.
- **Automations.** Schedule agent runs with a custom cron UI, reuse sessions, browse run history with pagination and filtering, manage external automation jobs, and drive it all from the CLI.
- **Quick Commands.** Reusable terminal and agent-prompt presets — scoped per context, searchable, bindable to keys.

## Editor, terminal, and the small things

- **An editor that earns its place.** Monaco with custom font option and minimap, a rich markdown editor (tables, toggle blocks, annotations, spellcheck, PDF export), plus viewers for mermaid, PDF, CSV/TSV, notebooks, and images with pinch zoom.
- **A terminal with taste.** Ghostty and Warp theme import (with preview), ligatures, Nerd Font fallback, configurable line height and contrast floor, OSC 52 clipboard, kitty keyboard protocol, IME fixes, shell integration, hyperlink action popovers, and a persistent daemon so sessions survive restarts.
- **Cmd+J everything.** One palette for tabs, worktrees, browser tabs, recent chats, issues, emulators, and create actions — ranked by recency and direct match.
- **Your keys, your way.** Fully customizable keyboard shortcuts, double-tap modifier chords, Ctrl+Tab MRU switcher, per-agent launch bindings, and Mission-Control conflict warnings.
- **Voice dictation.** Local Parakeet/SenseVoice/Zipformer models (English, Korean, Japanese) or OpenAI cloud transcription, with a sound-reactive visualizer.
- **AI Vault.** Searchable session history across all your agents — every conversation, indexed and queryable, with the first prompt shown per session.
- **`orca.yaml` + the CLI.** Setup scripts, trusted hooks, and default tab templates per repo — and a self-describing CLI (`orca terminal create`, `orca skills install`, `orca account add`, `orca linear`) that lets agents drive Orca itself.
- **Speaks your language.** UI localization in Korean, Japanese, French, and Simplified Chinese, with native-language search terms for settings.

## The pitch

Five months, ~265 releases: Orca went from "a nice way to run a few agents" to a full agent cockpit — any agent, any repo, any host, from your desk or your phone, with orchestration, review, and history layered on top. The agents change every month; the cockpit is the thing you keep.
