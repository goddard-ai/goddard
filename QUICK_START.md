# QUICK START (End-to-End Human Test)

This guide walks you through a full local end-to-end test of the current MVP.

You will:
1. Start the backend server
2. Log in with the CLI
3. Create a PR intent
4. Trigger an action intent
5. Open a live stream in the terminal
6. Send a simulated GitHub webhook and watch it appear in the stream

---

## Prerequisites

- Node.js 20+ (Node 22 recommended)
- `pnpm` installed
- You are in this repo root

```bash
pwd
# should end with /pi-loop
```

---

## 1) Install dependencies

```bash
pnpm install
```

Optional sanity check:

```bash
pnpm run check
```

---

## 2) Start the backend (Terminal A)

In **Terminal A**:

```bash
pnpm --filter @goddard-ai/backend start
```

Expected: backend starts on `http://127.0.0.1:8787`.
Keep this terminal running.

---

## 3) Log in via CLI (Terminal B)

Open **Terminal B** in repo root.

```bash
pnpm --filter @goddard-ai/cmd exec goddard login --username <your-github-name>
```

Example:

```bash
pnpm --filter @goddard-ai/cmd exec goddard login --username alec
```

Expected output:

```text
Logged in as @alec
```

Verify session:

```bash
pnpm --filter @goddard-ai/cmd exec goddard whoami
```

Expected output:

```text
@alec (id:...)
```

---

## 4) Create a PR intent

```bash
pnpm --filter @goddard-ai/cmd exec goddard pr create \
  --repo owner/repo \
  --title "Test PR from quick start" \
  --head feature/quickstart \
  --base main
```

Expected output:

```text
PR #1 created: https://github.com/owner/repo/pull/1
```

(If you run it again, PR number increments.)

---

## 5) Trigger an action intent

```bash
pnpm --filter @goddard-ai/cmd exec goddard actions trigger \
  --repo owner/repo \
  --workflow ci \
  --ref main
```

Expected output:

```text
Action queued: run <id> (ci on main)
```

---

## 6) Start live stream (Terminal C)

Open **Terminal C** and run:

```bash
pnpm --filter @goddard-ai/cmd exec goddard stream --repo owner/repo
```

Expected first line:

```text
Streaming owner/repo. Press Ctrl+C to exit.
```

Leave this running.

---

## 7) Simulate a GitHub webhook (Terminal B)

With stream still open in Terminal C, send a webhook payload:

```bash
node --input-type=module <<'EOF'
import { createGitHubApp } from "@goddard-ai/github-app";

const app = createGitHubApp({ backendBaseUrl: "http://127.0.0.1:8787" });

const result = await app.handleWebhook({
  type: "issue_comment",
  owner: "owner",
  repo: "repo",
  prNumber: 1,
  author: "teammate",
  body: "Looks good"
});

console.log(result);
EOF
```

Expected in **Terminal C** (stream): JSON event printed with `"type":"comment"` and `"reactionAdded":"eyes"`.

---

## 8) Test review webhook (optional)

```bash
node --input-type=module <<'EOF'
import { createGitHubApp } from "@goddard-ai/github-app";

const app = createGitHubApp({ backendBaseUrl: "http://127.0.0.1:8787" });

await app.handleWebhook({
  type: "pull_request_review",
  owner: "owner",
  repo: "repo",
  prNumber: 1,
  author: "reviewer",
  state: "approved",
  body: "Ship it"
});

console.log("review webhook sent");
EOF
```

Expected in stream: JSON event with `"type":"review"`.

---

## 9) Logout / cleanup

In Terminal B:

```bash
pnpm --filter @goddard-ai/cmd exec goddard logout
```

Stop stream with `Ctrl+C` in Terminal C, and stop backend with `Ctrl+C` in Terminal A.

---

## Troubleshooting

- **`Not authenticated. Run login first.`**
  - Run the `login` command again, then `whoami`.

- **Port already in use (`8787`)**
  - Stop existing process using that port, then restart backend.

- **`Unable to infer repository. Pass --repo owner/repo.`**
  - Always pass `--repo owner/repo` in this quick start.

- **Malformed event / stream issues**
  - Restart backend and stream terminals, then retry step 7.
