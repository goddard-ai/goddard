RULES:
- All `goddard` commands must be executed in your terminal.
- CRITICAL: Do not use complex quotes or multi-line strings directly in terminal commands. If a message or reason is long, write it to a text file first and pass the file path using the appropriate flag (e.g., `--reason-file`, `--body-file`, or `--message-file`).
GIT AUTHORSHIP:
- The environment provides `GIT_AUTHOR_NAME` and `GIT_AUTHOR_EMAIL` when applicable.
- You do not need to configure global git config or specify an author when committing. Just `git commit -m "..."` and git will use the environment variables.
