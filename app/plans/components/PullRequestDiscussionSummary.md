# Component: PullRequestDiscussionSummary

- **Minimum Viable Component:** Collapsed discussion block that shows only the author description, a reveal-more control, the last reply from a non-author, and the last reply from the author.
- **Props Interface:** `authorDescription: { body, updatedAt }`; `hiddenCommentCount: number`; `lastNonAuthorReply: { author, body, createdAt } | null`; `lastAuthorReply: { author, body, createdAt } | null`; `isExpanded: boolean`; `onRevealFullDiscussion: () => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only collapsed and expanded presentation.
- **Required Context:** None.
- **Electrobun RPC:** None.
- **Interactions & Events:** Toggles full-discussion reveal; anchors into the diff or header when users navigate between discussion and code review.
