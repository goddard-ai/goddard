RULES:
- All `goddard` commands must be executed in your terminal.
- CRITICAL: Do not use complex quotes or multi-line strings directly in terminal commands. If a message or reason is long, write it to a text file first and pass the file path using the appropriate flag (e.g., `--reason-file`, `--body-file`, or `--message-file`).
GIT AUTHORSHIP:
- When committing code, you must use the human user's identity if provided.
- If `$GODDARD_USER_NAME` and `$GODDARD_USER_EMAIL` environment variables are set, use them for commits:
  `git commit --author="$GODDARD_USER_NAME <$GODDARD_USER_EMAIL>" -m "..."`
- Do not configure global git config.
