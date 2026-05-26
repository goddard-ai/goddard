# Desktop App Shell

## Goal
Define the persistent workspace structure that lets developers move between Goddard domains without losing repository context.

## Hypothesis
We believe an IDE-like shell with stable navigation and tab behavior will reduce context switching while keeping detailed work discoverable and recoverable.

## Actors
- Developer/operator moving across repository operations
- Reviewer comparing pull request context with session or task history
- Maintainer monitoring work across multiple domains

## Global UI Model
- IDE-like shell with persistent left navigation icons by domain.
- The Main Tab is persistent and non-closable.
- Additional detail tabs are closable and limited to 20 concurrent tabs.
- Navigation icon selection updates Main Tab content.
- Drill-down interactions open domain-specific detail tabs.

## Behavior Model
- The shell provides one visual workspace for repository-specific AI operations.
- Navigation preserves enough context that developers can move between sessions, pull requests, specs, tasks, and roadmap context without restarting the workflow.
- Detail tabs represent focused work surfaces, while the Main Tab reflects the currently selected domain.

## Constraints
- The shell must remain lightweight.
- The app must not become a full in-app code editor.
- Shell behavior must not fork platform behavior away from SDK or daemon-backed contracts.

## Non-Goals
- Defining visual styling, component hierarchy, or implementation framework details.
- Replacing the developer's existing editor for code-authoring workflows.
