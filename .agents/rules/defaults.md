# Defaults

Read this ruleset when adding or changing config, normalization, persisted data shapes, SDK inputs, domain defaults, or resolved values.

- Treat behavior-affecting defaults as resolution-boundary logic, not local convenience.
- Put default config, persisted data, SDK inputs, and domain values only in named resolver, factory, normalization, or config-loading modules.
- Once a value crosses into a resolved type, do not default it again downstream.
- UI-only presentation fallbacks are allowed when they do not affect shared behavior, persistence, SDK behavior, or system configuration.
- Prefer separate raw and resolved shapes, such as `RawConfig` with optional fields and `ResolvedConfig` with required fields.
- Keep default constants close to the resolver that owns them, and name them for the specific behavior they control.
- Represent intentional absence with an optional field or explicit union instead of encoding it as a magic default value.
- Resolver tests should cover precedence across input sources and assert source metadata when provenance is kept for debugging.
