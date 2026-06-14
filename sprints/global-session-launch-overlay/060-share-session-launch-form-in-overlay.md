# 060-share-session-launch-form-in-overlay

## Objective

Reuse the existing session launch form in the overlay without creating a divergent launch flow.

## Scope

- Share launch form host/composition between in-app dialog and overlay where practical.
- Preserve selector commands, slash suggestions, adapter/model/location controls, and validation.
- Implement overlay project default resolution.
- Preserve main-dialog compatibility.

## Acceptance Criteria

- Overlay launch form behaves like the in-app dialog except for documented overlay-specific hide/reset/submit behavior.
- Project defaults resolve as last overlay project, main active project, then first project.
- The in-app launch dialog still works.

