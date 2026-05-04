# `sprint-branch finalize [--override-base <ref>]`

`finalize` answers "is the fully approved sprint ready for the human's final
merge?"

It prepares the completed review branch for landing. Finalization requires no
active review task, no next task, and no finished-unreviewed tasks. Review and
approved must represent the same approved content, and `next` must not contain
different work. The working tree must be clean, and the base ref must resolve.

The command brings the completed review content onto the sprint's recorded base,
updates the approved boundary to match review, and leaves the review branch as
the branch humans land from. `--override-base` exists for recovery when the
recorded base is not the target humans intend to land onto.

If a finalize conflict occurs, sprint state remains at the pre-finalize boundary
until the conflict is resolved and `finalize` is retried.

Why it matters: approval finishes the task queue; finalization prepares the
whole approved sprint for a clean human landing.
