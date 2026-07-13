# Session Profiles

## Goal

Let developers move between routine, debugging, and deep-reasoning session configurations without repeatedly selecting each agent option.

## Profile Model

- Each agent harness may have one `Routine`, one `Debug`, and one `Deep` profile.
- Each configured profile selects the harness model, thinking level, and approval mode together.
- Profiles record explicit options advertised by the agent harness. Goddard must not infer price, capability, or tier from model names, descriptions, ordering, or providers.
- Profile definitions are global user preferences. Repository configuration must not define or override them.
- Profile definitions must be available through the SDK and the desktop app from the same shared configuration source.

## Selection Behavior

- Session launch and active session chat must offer every configured profile that is valid for the selected agent harness.
- Selecting a profile applies its complete configuration before the next prompt and keeps that configuration until the developer changes it.
- Existing model, thinking-level, and approval-mode controls remain available.
- A profile is active only when the live agent configuration matches every selection in that profile. Manual changes may match another profile or leave no profile active.
- Changing a stored profile must not reconfigure an active session until the developer selects that profile in the session.

## Unavailable Profiles

- An unconfigured profile is not offered for selection.
- A profile becomes unavailable when its agent harness no longer advertises any recorded option.
- An unavailable profile must remain visible in management surfaces so the developer can repair or remove it.
- Goddard must not apply part of an unavailable profile or substitute another model, thinking level, approval mode, or profile.

## Management

- Developers must be able to configure and remove each fixed profile independently for an installed agent harness.
- Profile management must use current options advertised by the selected agent harness rather than a provider-specific model catalog.
- A failed profile update must preserve unrelated global configuration and the last valid profile definitions.

## Non-Goals

- Repository-scoped profiles.
- Arbitrary profile names or additional profile slots.
- Automatic model routing, price discovery, or capability classification.
- Model-initiated escalation or model changes during an in-flight inference.
- Delegating work to another session when a profile changes.
- Persisting a separate active-profile value on a session.
