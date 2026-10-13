# Employee purpose groups

Use explicit assignment purpose to separate lasting changes from supporting work
in the Boss sidebar. Purpose is persisted on the employee and survives restart,
expiry, and re-admission.

| Purpose | Work | Sidebar |
| --- | --- | --- |
| `building` | Implementation, features, fixes, lasting changes | Building |
| `helping` | Research, reviews, checks, monitoring, chores | Helping |
| Missing on an older record | Unknown until explicitly classified | Employees |

## Assignment controls

Supply `--purpose building|helping` to `goddard-agent boss summon`. Alternatively,
supply `--assignment-kind implementation|research` explicitly: implementation
defaults purpose to Building, research to Helping. Explicit purpose wins. A chore
can therefore use `--assignment-kind implementation --purpose helping`; purpose
never changes access, plan approval requirements, permissions, or reporting.

The protocol summon accepts the optional camelCase `purpose` field. A new
admission with neither purpose nor explicit `assignmentKind` fails with an
instruction to supply one. The fields remain optional for decoding older wire
payloads. Matching retries of old admissions keep their original employee and
unknown purpose, including their older request fingerprints.

Classify an existing employee using its roster ID:

```sh
goddard-agent boss employee purpose EMPLOYEE_ID helping
```

`EMPLOYEE_ID` is the employee UUID from `boss roster --all`. The corresponding
assignment operation is `control(sessionId, #{type:"setPurpose", purpose:"helping"})`.
It persists only purpose and works during queued, dispatching, working, finishing,
and expired states. Ownership checks still apply; classification does not
interrupt or resume work.

## Legacy behavior and disclosure

Missing purpose stays missing when loading persisted records. Do not infer it
from titles, personas, an AI classifier, workGoal, outcome/chain finisher status,
or the defaulted implementation access mode. New employee chains require `purpose` on every step definition; each dispatched
employee inherits that explicit purpose while keeping the chain’s implementation
access. New or revised definitions without purpose are rejected. Older persisted
chain definitions without classification still dispatch into Employees, so
existing work remains visible and can be classified with the same control.

Building and Helping reuse the former Outcomes and Errands disclosure tokens,
respectively, so existing collapse choices survive. Employees has its own saved
collapse state and finished-tail reveal counter. All three groups preserve
row status, attention markers, archive filtering, and the existing finished-tail
fold. `workGoal` continues to own legacy reporting and retention independently.
