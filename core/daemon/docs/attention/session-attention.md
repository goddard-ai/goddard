# Session Attention

A daemon-managed session can ask for human attention when agent work blocks or reaches a useful stopping point. This page explains how those reports update the inbox and how short metadata helps clients present them clearly.

## Core idea

- A daemon-managed session can refresh inbox attention when it needs human awareness or help.
- Session attention is current workflow state, not a second visible chat response.

## Blockers

- A session reports a blocker when it cannot make useful progress without help.
- Reporting a blocker marks or refreshes the session's inbox row as needing attention.
- The detailed reason belongs in the daemon report so clients can show useful context.
- The session may remain live, idle, or later become history-only; the inbox row describes the user's attention workflow rather than the process lifecycle by itself.

## Turn-ended updates

- A session can report that a turn reached a meaningful stopping point.
- This creates or refreshes attention when no other entity already claimed responsibility for the turn's attention.
- If pull request creation or another entity already owns the attention for the turn, the daemon should avoid creating duplicate session attention for the same user decision.

## Metadata

- Scope is a short, semi-stable label for the current work area.
- Headline is a short update about what changed or why attention matters now.
- The preferred mental model is `{scope} - {headline}`.
- Metadata helps clients group and sort current work, but users should still be able to understand the attention item from the visible session context.

## Boundaries

- Hidden attention metadata is not part of the visible user-facing chat response.
- Completing the session's inbox concern is separate from shutting down the session.
- A user reply can move a row to `replied` without making the session complete.
