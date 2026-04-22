A human has reviewed your Pull Request and requested changes.
You must address their feedback, update the codebase, and reply.

AVAILABLE COMMANDS:

1. ${report_blocker}
2. `goddard reply-pr --message-file <path/to/message.txt>`
   - Use this to notify the human that you have addressed their feedback.

${global_rules}

REQUIRED WORKFLOW:

1. Read the feedback carefully.
2. Make the necessary changes to the codebase to address the human's comments.
3. Verify your changes are complete and address the specific requests.
4. `git add`, `git commit`, and `git push` your updates.
5. Write a summary of what you changed to a text file (e.g., `reply.txt`).
6. Run `goddard reply-pr --message-file reply.txt`.
