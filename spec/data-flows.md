---
id: system-data-flows
status: ACTIVE
links:
  - type: Extends
    target: spec/architecture.md
  - type: Relates-To
    target: spec/cli/interactive.md
  - type: Relates-To
    target: spec/daemon/pr-feedback-one-shot.md
---

# Data Flows

## PR Creation (Interactive Mode)

End-to-end sequence from developer command to GitHub PR creation and live event receipt.

```
1.  Dev runs:   goddard pr create -m "Fix memory leak"
2.  CLI → SDK:  format and validate PR creation request
3.  SDK → Worker: POST /pr  (with session token)
4.  Worker → Turso: validate session, fetch GitHub identity
5.  Worker → GitHub API: create PR as goddard[bot], append "Authored by @username"
6.  Reviewer: comments on PR at github.com
7.  GitHub → Worker: POST webhook (issue_comment event)
8.  Worker → Octokit: add 👀 reaction to comment
9.  Worker → Durable Object: route event payload to repo's Durable Object
10. Durable Object → SSE stream: broadcast to all subscribed CLI clients for that repo
11. SDK: parse broadcast frame, emit typed `comment` event
12. CLI: print comment natively in terminal
```

---

## Authentication (Device Flow)

Sequence for the initial `goddard login` flow.

```
1. CLI → Worker: POST /auth/device — request a device code
2. Worker → Turso: create pending cli_session record
3. Worker → CLI: return device_code, user_code, verification_uri
4. CLI → User: display user_code and open verification_uri in browser
5. User → GitHub: authorize the Goddard GitHub App
6. GitHub → Worker: OAuth callback with authorization grant
7. Worker → Turso: mark cli_session as authorized, store GitHub identity
8. CLI: polls Worker until session is authorized
9. Worker → CLI: return session token
10. CLI → TokenStorage: persist session token to ~/.goddard/config.json
```

---

## Real-Time Event Stream (Daemon Subscription)

Sequence for daemon PR-feedback automation.

```
1. Daemon → SDK: subscribeToRepo("owner/repo")
2. SDK → Worker: GET /stream?owner=...&repo=...&token=...  (Accept: text/event-stream)
3. Worker: validates session, routes to Durable Object for that repo
4. Durable Object: registers SSE connection
5. GitHub event fires (e.g., new PR comment)
6. GitHub → Worker: POST webhook
7. Worker → Durable Object: forward event payload
8. Durable Object → SSE stream: broadcast typed JSON frames to all subscribers
9. SDK: parse frames, validate shape, emit typed `comment` / `review` / `error` event
10. Daemon: verifies PR is Goddard-managed
11. Daemon: launches one-shot local `pi` session with PR feedback context
```

---

## Autonomous Cycle (Loop Mode)

Simplified sequence for a single agent cycle within the loop runtime.

```
1. Loop: apply rate limiting (cycleDelay + maxOpsPerMinute)
2. Loop → Strategy: call nextPrompt({ cycleNumber, lastSummary })
3. Loop → pi-coding-agent session: send prompt
4. pi-coding-agent: execute against codebase, optionally call SDK to create PRs
5. Loop: read session token delta, enforce maxTokensPerCycle
6. Loop: extract lastSummary from agent response
7. Loop: check for DONE signal → terminate cleanly, or continue to next cycle
```
