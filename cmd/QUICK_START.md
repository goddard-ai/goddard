# QUICK START (Production)

This guide walks you through using the production version of Goddard.

Goddard allows you to create Pull Requests, trigger GitHub Actions, and receive real-time streaming updates about repository events directly in your terminal.

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

## 5. Live Stream Repository Events

Watch real-time events (comments, reviews, CI status) in your terminal:

```bash
goddard stream
```

Keep this running to see live updates as they happen on GitHub.

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
