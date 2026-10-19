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

## Create a chain by name or root

Chain creation and revision accept exactly one project selection per step:
`"projectId": "REGISTERED_UUID"`, `"project": {"name": "goddard"}`, or
`"project": {"path": "/absolute/registered/root"}`. Keep `projectId` UUID-only.
Name/path selections use the discovery rules above and reject temporary
registrations. UUID selections retain legacy temporary-project behavior.

These commands require a daemon with chain selector support. Older daemons
reject the new fields; upgrade or resolve the project and use `projectId`.

1. List or resolve the project using the commands above. Obtain an open outcome
   UUID from `goddard-agent boss view`, and choose an installed provider model.
2. Save this complete create operation as `chain.json`. Replace the illustrative
   outcome UUID and project name with your actual values; choose a new request
   UUID once for this logical creation. This one-step helping chain does not
   claim outcome completion.

   ```json
   {
     "type": "create",
     "requestId": "22222222-2222-4222-8222-222222222222",
     "definition": {
       "outcomeId": "33333333-3333-4333-8333-333333333333",
       "finishesOutcome": false,
       "steps": [{
         "purpose": "helping",
         "title": "Inspect the project",
         "prompt": "Read the project instructions and report the package layout. Make no changes.",
         "project": {"name": "goddard"},
         "personaId": null,
         "provider": "codex",
         "model": "gpt-5.5",
         "reasoningEffort": null,
         "permissions": {"bucketIds": [], "integrationIds": [], "summonEmployees": false, "computerUse": false},
         "resources": {},
         "workspace": "local",
         "baseRevision": null,
         "boundary": "artifactsReady",
         "requiredChecks": [],
         "requiredArtifacts": [],
         "requiredApprovals": [],
         "maxRetries": 0,
         "planId": null,
         "itemId": null,
         "successorBrief": ""
       }]
     }
   }
   ```

3. Run `goddard-agent boss chain --json-file chain.json`. For a root selection,
   replace the entire `project` object with `{"path": "/absolute/registered/root"}`.
   Shell expansion does not happen inside JSON; supply the full absolute path.
4. Inspect the returned chain, or save
   `{"type":"show","chainId":"ACTUAL_CHAIN_UUID"}` as `show.json` and run
   `goddard-agent boss chain --json-file show.json`. Each accepted step's
   `definition.projectId` is its execution identity. `projectMetadata` shows
   the current catalog name/path, or `null` if that ID is no longer registered.
   The chain's `request` preserves the submitted selector for replay comparison.

All steps resolve and validate before any are accepted. An unknown or ambiguous
later step leaves no chain, employee, or outcome-finisher claim. Errors identify
the 1-based failing step and include a stable code and next action. For
`project_ambiguous`, choose a candidate's ID or registered root, update the
failed definition, and retry with the same request ID. Do not repeatedly submit
an unchanged ambiguous selection. A failed preflight does not reserve the ID.

For a malformed UUID or memory-derived identifier, run project discovery and
copy the canonical UUID, or use a name/root selector. Never reconstruct project
UUIDs from memory bucket metadata.

## Revise an unstarted step

Use a chain with at least two steps; the one-step creation example above has
no unstarted successor to revise. Pause the chain with a control operation
using its current `revision`. Only an unstarted step without attempts can be revised. Save this operation as
`revise.json`, replacing the illustrative chain UUID, revision numbers, registered root,
and model with values from your chain and catalog:

```json
{
  "type": "revise",
  "chainId": "44444444-4444-4444-8444-444444444444",
  "expectedRevision": 4,
  "expectedDefinitionVersion": 1,
  "step": 1,
  "definition": {
    "purpose": "helping",
    "title": "Inspect the project",
    "prompt": "Read the project instructions and report the package layout. Make no changes.",
    "project": {
      "path": "/Users/alec/dev/goddard-ai/goddard"
    },
    "personaId": null,
    "provider": "codex",
    "model": "gpt-5.5",
    "reasoningEffort": null,
    "permissions": {
      "bucketIds": [],
      "integrationIds": [],
      "summonEmployees": false,
      "computerUse": false
    },
    "resources": {},
    "workspace": "local",
    "baseRevision": null,
    "boundary": "artifactsReady",
    "requiredChecks": [],
    "requiredArtifacts": [],
    "requiredApprovals": [],
    "maxRetries": 0,
    "planId": null,
    "itemId": null,
    "successorBrief": ""
  }
}
```

The replacement is a complete step object. `step` is zero-based, so `1` selects the second step. Run
`goddard-agent boss chain --json-file revise.json`. A failed replacement leaves
the accepted definition intact; a stale revision requires inspecting the chain
and reviewing current state before retrying.

## Scripts and retries

Boss scripting uses the same authoritative endpoint:

```sh
goddard-agent boss script --text 'op(#{type: "chain", action: #{type: "show", chainId: "ACTUAL_CHAIN_UUID"}})'
```

For create or revise, pass the same complete operation under `action`; Rhai
maps use `#{...}` and null is `()`. Typed clients send `BossOperation::Chain`
with the same operation. Neither surface bypasses resolution or owner checks.

After a lost create response, retry the **unchanged definition, request ID, and
supervisor**. Accepted replay returns the original chain and canonical bindings
even after rename, removal, or re-registration. Changing a selector to an
equivalent UUID still conflicts with an accepted request ID. Revisions do not
change the original replay key.

A later missing registration or directory pauses execution with
`project_unavailable` in the primary reason and attempt history. Restore the
original registration/directory or explicitly revise an eligible step. A
similarly named project or a new registration at the same path never replaces
the bound identity automatically. Existing UUID payloads and stored chains keep
working.
