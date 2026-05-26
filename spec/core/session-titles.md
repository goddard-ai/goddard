# Session Titles

## Goal
Give every daemon session a short, durable, human-readable title so SDK consumers, the desktop app, and operational clients can identify session work without exposing raw identifiers or long prompt text.

## Hypothesis
We believe that stable session titles will make session lists, tabs, inbox references, and review surfaces easier to scan, while optional model-backed refinement can improve readability without making session creation slower or less reliable.

## Primary Actors
- Developer or operator scanning session history
- Runtime host creating or displaying daemon sessions
- Daemon maintaining durable session records
- SDK or app consumer presenting session state

## Behavior Model
- Every daemon session must have a non-empty title as soon as it is visible to clients.
- A session with no usable prompt text may use a generic placeholder title.
- Once user prompt text is available, Goddard must provide a deterministic fallback title without requiring external model access.
- When title generation is configured and usable, Goddard may refine the fallback title asynchronously.
- Title generation must not block session creation, session prompting, or normal session lifecycle transitions.
- Title generation failure must preserve the visible fallback title and must not make the session unusable.
- Hosts should display the session title supplied by the shared session record rather than deriving their own title from local UI context.

## Title States
- **Placeholder**: The session is visible before user prompt text is available, so the title is generic.
- **Fallback**: The title was derived locally from user prompt text and no generation attempt is currently expected to replace it.
- **Pending Generation**: The fallback title is visible while optional background refinement is in progress.
- **Generated**: The visible title was produced by configured background title generation and accepted as valid.
- **Generation Failed**: A generation attempt failed or produced an unacceptable title, so the fallback title remains visible.

## Configuration
- Title generation is an operator preference, not task semantics.
- Title generation configuration belongs to the shared configuration hierarchy used by the daemon, SDK, and app.
- Local project configuration may override the user's global title-generation default.
- Session title generation must be configured independently from the session agent's own model settings.
- Runtime overrides for one individual session are out of scope for title generation unless a later spec explicitly adds them.

## UX Requirements
- The desktop app should remain usable without title-generation configuration.
- The app may invite users to configure title generation after they encounter fallback titles, but it must not block first-run onboarding or session creation on title-generation setup.
- Any title-generation setup surface must make clear that it affects background session naming, not the model used by the agent to do work.
- Hosts may show a subtle pending affordance while generation is in progress, but the fallback title remains the primary visible title during that state.

## Non-Goals
- Manual session renaming.
- Repeated automatic retitling throughout a session's life.
- Reusing the session agent model setting for title generation.
- Creating additional daemon sessions or agent conversations only to name a session.
- Persisting provider credentials in Goddard configuration.

## Decision Memory
- Fallback titles are required because titles are part of the shared session experience, not a best-effort app decoration.
- Optional generated titles improve scanability, but they are auxiliary and must never become a prerequisite for local-only use.
- Title generation uses separate configuration because the model that names work is not necessarily the model that performs work.
