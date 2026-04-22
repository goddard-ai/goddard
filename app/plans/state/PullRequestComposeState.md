# State Module: PullRequestComposeState

- **Responsibility:** Manage creation and reply drafts for managed pull requests.
- **Data Shape:** One create-dialog draft; reply drafts keyed by pull request ref; validation errors; submission status; transient success and error state; protected-action requirements for create or reply workflows.
- **Mutations/Actions:** `openCreatePullRequestDialog`; `closeCreatePullRequestDialog`; `setCreatePullRequestDraft`; `submitPullRequest`; `setReplyDraft`; `submitReply`; `clearComposeError`.
- **Scope & Hoisting:** Hoisted into a shared provider because create and reply workflows are triggered from both page-level and detail-tab contexts.
- **Side Effects:** Uses protected auth flows when external identity is required; submits managed pull requests and replies through shared SDK-backed services; opens or refreshes pull request tabs after successful submission.
