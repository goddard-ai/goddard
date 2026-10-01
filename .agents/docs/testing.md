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

- Run scoped tests routinely during development, not only before finishing.
  On machines with mbx configured, `cargo test -p <changed crate>` restores
  dependency compilations from the shared cache even in a fresh worktree, so
  most of the cost is the changed crate and test execution. Prefer
  package-scoped or filtered runs; `--workspace` still builds every member's
  test binaries, including the app crate.

- Report verification gaps and provide concrete manual steps when automated verification is unavailable.

## Focused verification gaps

Source-text assertions cannot prove keyboard operation, layout, or the absence
of I/O through helpers. The requirements below remain in force. When changing
these areas, verify the affected contract in the rebuilt debug app and report
any checks that were unavailable. Do not treat these checks as automated coverage.

| Area | Focused check | Expected result |
| --- | --- | --- |
| Markdown code blocks | Use Tab to reach Run and Copy; activate each with Enter and Space. Paste copied multiline code into an editor. | Run receives the displayed code and language; Copy preserves its full text and newlines; focus is visible. |
| Code and diff wrapping | Open long code and diff lines, narrow the panel, and select across continuation rows. | Lines wrap without clipping; selection and copied content preserve the original text. |
| Panel and background titles | Use a multiline shell command as a background title; open its popover and collapse its row. | Titles stay on one line; full content remains available in the expanded surface. |
| Background expansion | Expand different background items, then expand an item that already has a tab. | The active surface retargets; existing tabs are activated without duplication. |

For changes to command palette, Review, Working tree, file editor, autocomplete,
or file finder rendering:

1. Review the renderer and every newly reachable helper. Filesystem access,
   subprocesses, network, synchronous IPC, and filtering the complete project
   belong in background preparation; rendering reads the prepared snapshot.
2. Open the affected surface in a large project. Repeat repaint, scroll, resize,
   and query interactions after initial loading. Use the counters and sampling
   procedure in [Performance](performance.md) to distinguish preparation from
   repeated frame work. On macOS, `sample <debug-app-pid> 5` captures call stacks.
3. Confirm expensive preparation runs only when its inputs change. Record any
   unexpected render-reachable work and the measured case. Sampling is a
   diagnostic; it does not prove that every render path is free of I/O.

## External prerequisites

- The controlled OpenCode HTTP-server test requires `python3` on PATH. A missing
  interpreter fails the check rather than reporting a pass.
- The pure sign-in invocation test supplies a Shuru path in an isolated child
  process without launching a VM. The daemon sign-in check also initializes
  sandbox assets and is explicitly ignored. With Shuru and provider assets
  installed, run `cargo test -p waku-core sandbox_sign_in_returns_the_guest_invocation -- --ignored`.
  This check may initialize/download sandbox assets; use a sandbox-enabled
  development environment.
- Sparkle protocol registration requires macOS and a packaged debug app. After
  the existing dev watcher has built the bundle, run
  `cargo test --bin goddard routing_user_driver_satisfies_sparkle_protocols -- --ignored`.
  Set `CARGO_TARGET_DIR` to the watcher's target directory if it is customized.
  This explicitly ignored check fails when the framework is absent; ordinary
  test runs do not verify the packaged framework's protocol compatibility.
