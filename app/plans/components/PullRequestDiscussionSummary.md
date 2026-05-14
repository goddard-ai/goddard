# Component: PullRequestDiscussionSummary

- **Minimum Viable Follow-On:** Collapsed discussion block for daemon-provided PR discussion data once the SDK exposes author text, comments, and timeline summaries.
- **Props Interface:** `authorDescription: { body, updatedAt } | null`; `hiddenCommentCount: number`; `lastNonAuthorReply: { author, body, createdAt } | null`; `lastAuthorReply: { author, body, createdAt } | null`; `isExpanded: boolean`; `onRevealFullDiscussion: () => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only collapsed and expanded presentation. Fetching or expanding timeline data belongs in `PullRequestState` only if query cache is not enough.
- **Required Context:** None.
- **Electrobun RPC:** None.
- **Interactions & Events:** Reveals full discussion, anchors between discussion and diff sections, and renders empty states when discussion data is unavailable.
