# Security Audit: Daemon Repo Stream Subscriptions

**Scope:** SSE subscription → Durable Object broadcast → daemon event handler  
**Question:** Does a daemon only receive events for PRs its own agent created?

**Answer: No.** Every authenticated daemon subscribed to `owner/repo` receives all events for that room — including PR content from other users' agents. The `isManaged` guard blocks *acting* on foreign PRs but not *receiving* their payloads.

---

## Vulnerabilities

### V1 — Broadcast is not user-scoped (High)

`RepoStream` holds a flat `Set<SseSession>`. Every frame is written to every subscriber regardless of who authenticated:

```ts
[...this.#sessions].map(async (session) => {
  await session.writer.write(payload); // all subscribers, no filter
})
```

Any valid Goddard user who knows an `owner/repo` slug receives the full content of all PR events — comments, review bodies, author identities — including those on PRs created by other users. High-impact on private repositories.

### V2 — `isManaged` does not check ownership (High)

The daemon guard checks "is this PR in the Goddard DB", not "did *my* agent create it":

```ts
const managed = await sdk.pr.isManaged({ owner, repo, prNumber });
```

Both `InMemoryBackendControlPlane` and `TursoBackendControlPlane` return `true` for any managed PR regardless of `createdBy`. Two daemons on the same repo both receive the event, both pass the guard, and both launch competing one-shot `pi` sessions against the same PR.

### V3 — Token in SSE query string (Low)

```ts
repoStreamRoute.GET({ query: { owner, repo, token } })
```

The bearer token appears in the GET URL. SSE connections are plain HTTP — more aggressively logged by proxies and access logs than WebSocket upgrades. Token exposure is persistent for the life of the connection.

---

## Fixes

### V1 + V2 — Scope sessions and ownership check to `githubUserId`

In `RepoStream`, replace `Set<SseSession>` with `Map<SseSession, string>` (session → userId). On broadcast, skip sessions whose userId doesn't match the PR's `createdBy`:

```ts
async broadcast(event: RepoEvent, createdBy?: string): Promise<void> {
  const payload = this.#encoder.encode(formatSseDataFrame(JSON.stringify({ event })));
  await Promise.allSettled(
    [...this.#sessions].map(async ([session, userId]) => {
      if (createdBy && userId !== createdBy) return;
      await session.writer.write(payload);
    })
  );
}
```

`PullRequestRecord.createdBy` is already in the DB — thread it through to the broadcast call. Strengthen `isManagedPr` with a `githubUsername` param and add it to the WHERE clause. The daemon has the session via `sdk.auth.whoami()` — pass it through.

### V3 — Ticket exchange instead of bearer token in URL

Add `POST /stream/ticket`: exchanges the bearer token for a short-lived (30s TTL) opaque ticket. The SSE URL carries only the ticket:

```
GET /stream?owner=...&repo=...&ticket=<opaque-id>
```

Backend redeems on first connection, associates the resolved `userId` with the session, and discards the ticket.

---

## Files Implicated

| File | Change |
|---|---|
| `backend/src/objects/RepoStream.ts` | `Set<SseSession>` → `Map<SseSession, string>`; filter by `createdBy` |
| `backend/src/index.ts` | Resolve `userId` on SSE admission; pass to Durable Object |
| `backend/src/control-plane.ts` | Add `githubUsername` param to `isManagedPr` interface |
| `backend/src/persistence.ts` | Filter `TursoBackendControlPlane.isManagedPr` by `createdBy` |
| `daemon/src/index.ts` | Pass `session.githubUsername` into `isManaged` call |
| `sdk/src/index.ts` | Replace token query param with ticket exchange |
