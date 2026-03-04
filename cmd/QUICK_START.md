# QUICK START (Production)

This guide walks you through using the production version of Goddard.

Goddard allows you to create Pull Requests, trigger focused AI sessions, and authenticate terminal workflows against the backend.

---

## 1. Install the CLI

The Goddard CLI can be installed globally via npm:

```bash
npm install -g @goddard-ai/cmd
```

Or you can use it directly via `npx`:

```bash
npx @goddard-ai/cmd --help
```

---

## 2. Login

Authenticate with your GitHub account:

```bash
goddard login --username <your-github-name>
```

This will initiate the GitHub Device Flow. Follow the instructions in your terminal to authorize the application.

---

## 3. Create a Pull Request

Navigate to a local git repository and run:

```bash
goddard pr create \
  --title "My feature implementation" \
  --body "This PR adds a new capability to the project." \
  --head feature/my-new-thing \
  --base main
```

Goddard will automatically infer the repository owner and name from your local git configuration.

---

## 4. Trigger a GitHub Action

Trigger a workflow in the current repository:

```bash
goddard actions trigger \
  --workflow ci.yml \
  --ref main
```

---

## 5. PR Feedback Automation (Daemon)

Repository event streaming is consumed by `@goddard-ai/daemon`, not the interactive CLI. Run the daemon from the monorepo root:

```bash
pnpm --dir=daemon daemon run --repo <owner/repo> --project-dir $(pwd)
```

The daemon listens for PR comments/reviews and launches one-shot `pi` sessions for managed PRs.

---

## 6. Autonomous Loop

Initialize and run the loop runtime:

```bash
goddard loop init
goddard loop run
```

Generate a systemd unit file:

```bash
goddard loop generate-systemd
```

---

## Troubleshooting

- **`Not authenticated. Run login first.`**
  - Run `goddard login` again.

- **`Unable to infer repository.`**
  - Ensure you are in a directory with a valid `.git` configuration, or pass `--repo owner/repo` explicitly.

- **`Request failed (404)`**
  - Ensure the repository has the Goddard GitHub App installed.
