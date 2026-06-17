# App Data

Read this ruleset when touching app query cache, SDK/daemon-backed reads, mutations, invalidation, or shared app data access.

- Use the local query cache in `src/lib/query.ts` for shared SDK or daemon-backed reads.
- Query functions passed to `useQuery()` should be stable references, not inline per-render closures.
- Import the shared `queryClient` directly from `src/lib/query.ts` when a feature needs manual cache access. Do not surface it with Preact context.
- Prefer immediate destructuring of SDK response objects from `useQuery()` and `useQueries()` when practical, instead of carrying wrapper objects like `session.session` through render code.
- Prefer feature-local write helpers that pair one SDK mutation with the affected query invalidation.
- Do not add thin read wrappers around simple `goddardSdk` query calls. Prefer stable `goddardSdk` methods directly unless a helper adds real behavior such as nullable handling.
- Do not add optimistic UI or loading indicators for local form submissions.
- Import-path precedence in `app/src/` is `./...`, then `~/...`, then `../...`.
- Use explicit TypeScript source extensions on those imports. Prefer `.ts` for `.ts` modules and `.tsrx` for `.tsrx` modules, including `~/...` imports.
- Use `./...` for same-folder modules first.
- Use `~/...` for imports that would otherwise traverse up to `src/` or across feature roots.
- Use `../...` only when it does not traverse up to `src/` itself. A single `../...` is allowed when it still lands inside a child path such as `src/foo/...`, but do not use `../...` to reach `src/...` broadly.
- Never use `../../...` or deeper upward traversal imports.
