# Testing

- Maintain the smallest test portfolio that provides strong confidence in durable, consequential behavior.

- Every committed test must protect a valuable observable contract against a realistic regression. Do not add tests merely because code changed or coverage would increase.

- Prioritize consequential risks: security, permissions, persistence, data integrity, external boundaries, concurrency, resource cleanup, unsafe code, and project infrastructure.

- Prefer one test at the smallest boundary that proves the contract. Add overlapping or narrower coverage only for a distinct risk or materially better failure localization.

- Test behavior rather than implementation structure. Behavior-preserving refactors should not normally break tests.

- Do not introduce production abstractions or expose internals solely for testing.

- Keep local implementation real; substitute external dependencies when needed to control nondeterminism, cost, or availability. Keep tests deterministic and independent.

- Consider existing coverage, compiler guarantees, static checks, and focused manual verification before adding tests.

- Apply the same value threshold to regression tests, snapshots, property tests, compile-fail tests, and feature or platform matrices.

- Remove redundant, brittle, or low-value tests and temporary exploratory tests. Never weaken valuable coverage merely to make a change pass; change its expectations only when the contract intentionally changes.

- Understand failures before changing code or tests. Run focused checks, then broader affected checks as warranted.

- Report verification gaps and provide concrete manual steps when automated verification is unavailable.
