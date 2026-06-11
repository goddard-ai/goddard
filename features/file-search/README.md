# @goddard-ai/file-search

`@goddard-ai/file-search` owns daemon-backed file and folder entry discovery for
composer `@` suggestions. It exposes the `fileSearch` SDK namespace and the
`fileSearch.composerEntries` IPC route.

The feature uses `@ff-labs/fff-bun` for indexed queries, including empty query
strings. Native create/search failures fall back to deterministic filesystem
listing/search inside the daemon.

## Runtime Validation

`test/native-smoke.test.ts` imports `@ff-labs/fff-bun`, creates a `FileFinder`,
waits for scan readiness, and destroys the finder. Keep that smoke path passing
whenever the native package version or daemon packaging path changes.

For desktop packaging changes, also run `bun run --cwd app build` on the target
platform and manually launch the packaged app to verify composer `@` suggestions
from a real project. The Electrobun build stages an embedded daemon runtime, so
final resource-location validation is outside the package unit test boundary.
