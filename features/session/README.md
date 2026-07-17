# Session Feature

The session feature owns daemon-backed ACP session lifecycle, persistence, launch preparation, prompting, and shared daemon/SDK controls.

## Session Profiles

Session profiles are global user preferences that group one agent harness model, thinking level, and approval mode under the fixed `Routine`, `Debug`, and `Deep` slots.

Profiles store concrete semantic selections rather than inferred tiers. Their applicability and active state are always derived from the harness's current ACP config options, so a removed option makes a profile unavailable instead of selecting a fallback. Profile definitions do not change an active session until a client explicitly applies one through the existing session controls.
