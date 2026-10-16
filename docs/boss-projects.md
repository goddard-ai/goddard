# Boss project discovery

Boss workflows sometimes need a project's canonical registered identity — the
UUID, display name, and root path the daemon stores for it — not just the name
or path shown in context. `goddard-agent boss projects` reads that catalog.
It is read-only: it registers nothing, grants no access, and never picks a
"closest" project for you.

Every Boss prompt's work overview already lists projects by name; the `context`
operation's digest now shows each project's registered id alongside its name
and path. Use these commands when you need the identity without the digest, or
when a digest folded entries away.

## List the catalog

```sh
goddard-agent boss projects list
```

Returns every registered project visible to the caller, sorted by name:

```json
{"type": "projects", "projects": [
  {"id": "11111111-1111-4111-8111-111111111111",
   "name": "goddard",
   "path": "/Users/alec/dev/goddard-ai/goddard",
   "temporary": false}
]}
```

`temporary: true` marks an ad-hoc registration picked for a single task ("New
task in…"). It leaves the catalog once no live session references it, so prefer
a persistent project for anything meant to outlive that task.

## Resolve one project

```sh
goddard-agent boss projects resolve goddard
goddard-agent boss projects resolve --name goddard
goddard-agent boss projects resolve --path /Users/alec/dev/goddard-ai/goddard
goddard-agent boss projects resolve --id 11111111-1111-4111-8111-111111111111
```

A bare selector is read by shape: UUID text selects the id, absolute or
separator-bearing text selects a registered root path, and anything else is an
exact name. The explicit `--id`, `--name`, and `--path` forms select a kind
directly — use them when a project is named like a UUID or a path. Exactly one
form applies per call.

Matching is deliberately strict:

- **Names** match exactly, ignoring ASCII letter case only. There is no
  trimming, substring, fuzzy, or Unicode case-fold matching.
- **Paths** must be the absolute registered root. Redundant separators, `.`
  and `..` segments, and trailing separators are normalized, and existing
  symlinks resolve to the same canonical root. A subdirectory, a `~` or
  relative path, and an unregistered worktree never match.
- **An id** looks the record up directly — the same behavior explicit UUID
  selection always had, including for temporary registrations.

When the input is wrong the error names why, shows candidate records where
useful, and suggests the correction — `project_unknown` when nothing matches,
`project_ambiguous` when several do (with candidate ids and paths to choose
from), `project_selector_invalid` for malformed or relative input, and
`project_temporary` when a name or path selection lands on a temporary
registration. A memory bucket id such as `project-abc123…` is not a project id
and cannot select one; list the catalog for the canonical id instead.

Resolution is advisory: it identifies a project at that moment but reserves
nothing. Work that must keep a specific identity should carry the returned id.

## Who can use it

Boss, planning sessions, and the human share this catalog — the same authority
as the `context` work digest. Employees cannot discover the catalog; an
employee holding a summon-time Boss-authority grant keeps it. Selecting a
project grants that project no extra permissions and no memory-bucket access.

## Planned: chain step selectors

Chain definitions today still require a per-step project UUID. A follow-up
change will let each step name its project by `{"projectId": "…"}` or a tagged
`{"project": {"name": "…" | "path": "…"}}` selector resolved against this same
catalog and rules. Until that lands, `projects resolve` is how a caller turns a
known name or path into the UUID a chain step needs.
