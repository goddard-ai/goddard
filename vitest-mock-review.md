# Vitest Mock API Usage Review

Date: 2026-03-22

This document reviews the current usage of Vitest's mock API (`vi.mock`, `vi.fn`, `vi.spyOn`, `vi.hoisted`) across the `goddard` workspace. It highlights areas where test mocks are being misused or abused, potentially leading to fragile tests, false positives, and maintenance burdens, and provides actionable recommendations to improve the test suite.

## Top Findings & Misuses

### 1. Large, Duplicated Inline Mocks for Shared Packages
The most significant abuse of the mock API is the widespread usage of large, inline `vi.mock` configurations to fake complex stateful dependencies like `@goddard-ai/storage` and `@goddard-ai/storage/session-permissions`.

**Examples:**
- `core/daemon/test/session-lifecycle.test.ts`
- `app/src/daemon-session.test.ts`

In both of these files, hundreds of lines of code are dedicated to re-implementing the semantic behavior of the persistence layer in memory using `vi.hoisted` Maps (e.g., `sessions`, `sessionStates`, `permissionsBySessionId`, etc.).

**Why this is harmful:**
- **Semantic Drift:** The mock implementations duplicate the exact behavior of the storage layer (e.g., handling timestamps, null defaults, and array pushes). If the real storage semantics change, the tests will continue to pass because they are testing against the frozen logic of the mock, leading to false confidence.
- **Maintenance Burden:** The same complex mock implementation is copied and pasted across multiple workspace packages. Updating a single storage contract requires hunting down and updating multiple inline test mocks.

### 2. Over-Mocking Standard Library Implementation Details
Certain packages are aggressively mocking standard Node.js APIs to assert on exact implementation details and internal control flow rather than observable behaviors.

**Example:**
- `core/worktree/test/worktree.test.ts`

This file uses `vi.mock("node:child_process")` and `vi.mock("node:fs")` to stub out `spawnSync` and `existsSync`. The tests intricately construct fake repository environments and check exact `spawnSync` invocation sequences (e.g., ensuring `git worktree add` or `rm -rf` are called).

**Why this is harmful:**
- **Brittle Tests:** These tests are tightly coupled to the internal mechanism of how a `Worktree` is constructed. Any refactoring of the command-line flags or process invocation will break the test, even if the end result is correct.
- **Low Confidence:** Passing these tests does not guarantee that the tool actually behaves correctly on a real file system, only that it issues the expected bash commands.

### 3. Overuse of `vi.hoisted` for Complex State
While `vi.hoisted` is necessary when sharing variables between test bodies and `vi.mock` factories (because imports are hoisted), using it to declare and share complex data structures (like global mutable Maps for mocked databases) makes test files difficult to read and reason about.

**Examples:**
- `core/daemon/test/session-lifecycle.test.ts` uses `vi.hoisted` to hoist 7 different mock definitions and state Maps.

### 4. Assertion Style Drift and Global Test State
Many packages that use Vitest test functions are drifting from the recommended `expect` assertions to Node's `assert` or relying on potentially leaking mock states. Tests use `vi.spyOn(process.stdout, "write")` to capture output without guaranteeing proper test isolation if a failure occurs mid-test (though `finally` blocks are used in some places).

## Recommendations

### Extract Shared Test Fixtures for Stateful Mocks
Instead of writing inline `vi.mock` implementations for `@goddard-ai/storage`, we should provide thin, reusable test fixtures or "in-memory" adapters directly from the package itself (e.g., exporting a `createMemoryStorage()` utility for testing). Tests in `app/` and `core/daemon/` can then inject this in-memory implementation rather than maintaining their own `vi.mock` logic.

**Actionable Step:**
1. Create a shared storage fixture package or test utilities inside `core/storage`.
2. Replace `vi.mock("@goddard-ai/storage")` calls with this shared fixture to centralize storage semantics and prevent drift.

### Prefer Contract Tests over Implementation-Detail Mocks
For modules interacting with the filesystem or spawning processes (like `core/worktree`), favor contract tests or integration tests with actual temporary directories (`fs/promises.mkdtemp`) over mocking `child_process` and `fs`.

If testing all failure conditions with a real filesystem is too slow, extract the "command execution" interface into a smaller, easily mockable boundary (e.g., a `CommandRunner` dependency) instead of globally mocking `node:child_process` which affects the entire test environment.

### Restrict `vi.hoisted` and Keep Mocks Lean
Avoid using `vi.hoisted` to build complex state machines inside test files. Mocks should ideally return static stubs or track simple call counts (`vi.fn()`). If a mock requires a complex state machine (like a database), that state machine should be a properly abstracted class or utility that is explicitly instantiated per-test, not globally hoisted.

### Centralize Global Setup and Teardown
Ensure that `vi.clearAllMocks()`, `vi.resetModules()`, and `vi.restoreAllMocks()` are consistently used in `beforeEach` or `afterEach` hooks. Centralize output capture logic (like spying on `stdout`) into safe, reusable test utilities to prevent log bleed between test cases on failure.

## Conclusion
The current Vitest test suite provides excellent behavioral coverage, but it relies too heavily on deep, complex inline mocks. By extracting shared test fixtures and shifting away from mocking standard library implementation details, we can significantly reduce test fragility and ensure that our test suite accurately reflects the actual contracts of our internal packages.
