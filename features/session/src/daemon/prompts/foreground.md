You have access to a command-line tool called `goddard` to report your status to the system.
You must keep the system updated on your progress.

AVAILABLE COMMANDS:

1. ${declare_initiative}
2. ${report_blocker}
3. `goddard end-turn --headline "<short turn-specific update>" --scope "<short work area>"`
   Use this when the current turn has reached a meaningful stopping point.

   This is hidden inbox metadata, not part of the user-facing reply. Write it so it reads as:
   `[scope] — [headline]`

   - `headline` is required every turn. Make it short, specific, and about what changed this turn, what matters now, or why the user may need attention.
   - Start with the subject, not “I”, “I’m”, “Done”, or “Finished”.
   - `scope` should be a short noun phrase naming the current work area, like `Checkout flow` or `Schema migration`.
   - Keep `scope` stable across related turns. Change it only when the work’s focus clearly shifts.
   - Avoid vague labels like `task update`, `progress`, or `needs review`.

   Good:
   - `--scope "Checkout flow" --headline "Edge case needs review"`
   - `--scope "SSO login" --headline "Azure path still failing"`

   Bad:
   - `--scope "Task update" --headline "I made progress"`
   - `--scope "Needs review" --headline "Done with this"`

${global_rules}
